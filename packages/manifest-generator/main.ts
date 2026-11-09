#!/usr/bin/env node
// Generates resources.json: captures GitHub releases into a snapshot, observes
// alpha versions through the manifest's own remote version plans, and compiles
// the template through @manifest/runtime (design §9.7).
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { isDeepStrictEqual, parseArgs } from "node:util";
import type {
  AssetSnapshot,
  ManifestSnapshot,
  ReleaseSnapshot,
  ReleaseSourceSnapshot,
  ResourceChecksum,
  ResourceManifest,
  ResourceManifestTemplate,
  SupportedPlatform,
} from "@manifest/metadata";
import {
  compileManifest,
  finishVersion,
  ManifestRuntimeError,
  planVersion,
  selectVariant,
  validateManifest,
  type RemoteResponseInput,
} from "@manifest/runtime";
import { getJson, getText, type HttpOptions } from "./http.ts";
import { summarize } from "./summary.ts";
import { TEMPLATE } from "./template.ts";

export const GITHUB_API = "https://api.github.com";
const PAGE_SIZE = 100;
const DIGEST = /^(md5:[0-9a-f]{32}|sha256:[0-9a-f]{64}|sha512:[0-9a-f]{128})$/;

export interface GenerateOptions extends HttpOptions {
  /** Directory receiving resources.json, snapshot.json and summary.md. */
  out: string;
  template?: ResourceManifestTemplate;
  /** GitHub REST API base URL. */
  api?: string;
  /** Run start time; written as observed_at and updated_at. */
  now?: Date;
  log?: (line: string) => void;
}

/** `id` fields arrive as decimal strings (see `getJson`). */
interface GithubRelease {
  id: string;
  tag_name: string;
  url: string;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
}

interface GithubAsset {
  id: string;
  name: string;
  digest?: string | null;
  updated_at: string;
}

interface Context {
  api: string;
  http: HttpOptions;
  log: (line: string) => void;
}

async function pages<T>(
  url: string,
  context: Context,
  stop: (items: T[]) => boolean,
): Promise<T[]> {
  const items: T[] = [];
  for (let page = 1; ; page++) {
    const separator = url.includes("?") ? "&" : "?";
    const batch = await getJson<T[]>(
      `${url}${separator}per_page=${PAGE_SIZE}&page=${page}`,
      context.http,
    );
    items.push(...batch);
    if (batch.length < PAGE_SIZE || stop(items)) return items;
  }
}

function asset(asset: GithubAsset, context: Context): AssetSnapshot {
  const digest = asset.digest?.toLowerCase();
  if (asset.digest && !DIGEST.test(digest!)) {
    context.log(
      `WARN ignored unsupported digest of ${asset.name}: ${asset.digest}`,
    );
  }
  return {
    id: asset.id,
    name: asset.name,
    ...(digest && DIGEST.test(digest)
      ? { digest: digest as ResourceChecksum }
      : {}),
    updated_at: asset.updated_at,
  };
}

async function release(
  repository: string,
  release: GithubRelease,
  context: Context,
): Promise<ReleaseSnapshot> {
  const assets = await pages<GithubAsset>(
    `${context.api}/repos/${repository}/releases/${release.id}/assets`,
    context,
    () => false,
  );
  return {
    id: release.id,
    tag: release.tag_name,
    url: release.url,
    ...(release.published_at ? { published_at: release.published_at } : {}),
    assets: assets
      .map((item) => asset(item, context))
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)),
  };
}

async function history(
  repository: string,
  limit: number,
  context: Context,
): Promise<ReleaseSnapshot[]> {
  const stable = (items: GithubRelease[]) =>
    items.filter((item) => !item.draft && !item.prerelease);
  const listed = await pages<GithubRelease>(
    `${context.api}/repos/${repository}/releases`,
    context,
    (items) => stable(items).length >= limit,
  );
  const releases: ReleaseSnapshot[] = [];
  for (const item of stable(listed).slice(0, limit)) {
    releases.push(await release(repository, item, context));
  }
  return releases;
}

async function tagged(
  repository: string,
  tag: string,
  context: Context,
): Promise<ReleaseSnapshot> {
  const item = await getJson<GithubRelease>(
    `${context.api}/repos/${repository}/releases/tags/${encodeURIComponent(tag)}`,
    context.http,
  );
  return release(repository, item, context);
}

function repository(
  template: ResourceManifestTemplate,
  resourceId: string,
): string {
  const source = template.resources[resourceId]!.source;
  if (!source?.owner || !source.repo) {
    throw new Error(
      `resource ${resourceId} needs source.owner and source.repo`,
    );
  }
  return `${source.owner}/${source.repo}`;
}

/** Captures every release source; tag sources have no observed version yet. */
async function capture(
  template: ResourceManifestTemplate,
  observedAt: string,
  context: Context,
): Promise<ManifestSnapshot> {
  const resources: ManifestSnapshot["resources"] = {};
  for (const [resourceId, resource] of Object.entries(template.resources)) {
    const repo = repository(template, resourceId);
    const sources: Record<string, ReleaseSourceSnapshot> = {};
    for (const [sourceId, source] of Object.entries(resource.releases)) {
      context.log(`capture ${resourceId}.${sourceId} from ${repo}`);
      sources[sourceId] =
        source.type === "history"
          ? {
              type: "history",
              releases: await history(repo, source.limit, context),
            }
          : { type: "tag", release: await tagged(repo, source.tag, context) };
    }
    resources[resourceId] = sources;
  }
  return { observed_at: observedAt, resources };
}

/**
 * Resolves every remote artifact's version as a host would and requires one
 * version per tag source. Returns resource ID → source ID → version.
 */
async function observe(
  template: ResourceManifestTemplate,
  provisional: ResourceManifest,
  context: Context,
): Promise<Map<string, Map<string, string>>> {
  const observed = new Map<string, Map<string, string>>();
  using manifest = validateManifest(provisional);
  for (const [resourceId, resource] of Object.entries(template.resources)) {
    const versions = new Map<string, string>();
    observed.set(resourceId, versions);
    for (const [index, variant] of resource.variants.entries()) {
      const variantId = provisional.resources[resourceId]!.variants[index]!.id;
      for (const [platform, artifact] of Object.entries(variant.artifacts)) {
        if (artifact.version.type !== "remote") continue;
        using selected = selectVariant(
          manifest,
          resourceId,
          platform as SupportedPlatform,
          { id: variantId },
        );
        using plan = planVersion(selected);
        const responses: RemoteResponseInput[] = [];
        for (const part of plan.value.remote!.parts) {
          const body = await getText(part.request.canonical_url, context.http);
          responses.push({ body, headers: [] });
        }
        const version = finishVersion(plan, responses);
        const sourceId = artifact.version.releases;
        const previous = versions.get(sourceId);
        if (previous !== undefined && previous !== version) {
          throw new Error(
            `${resourceId}.${sourceId}: artifacts report versions ${previous} and ${version}; upstream is publishing`,
          );
        }
        versions.set(sourceId, version);
      }
    }
    for (const [sourceId, version] of versions) {
      context.log(`observe ${resourceId}.${sourceId} = ${version}`);
    }
  }
  return observed;
}

/** Reads an existing output file; a missing file reads as absent. */
async function readText(path: string): Promise<string | undefined> {
  try {
    return await readFile(path, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    throw error;
  }
}

/**
 * Reads an existing JSON output file. A missing or malformed file, such as one
 * truncated by an interrupted write, reads as absent so it gets regenerated.
 */
async function readJson<T>(path: string): Promise<T | undefined> {
  const text = await readText(path);
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text) as T;
  } catch {
    return undefined;
  }
}

/**
 * Every static artifact's latest release must carry a digest; otherwise the
 * default selection resolves to checksum kind `none`, which no policy accepts.
 */
function requireLatestDigests(manifest: ResourceManifest): void {
  for (const [resourceId, resource] of Object.entries(manifest.resources)) {
    for (const variant of resource.variants) {
      for (const [platform, artifact] of Object.entries(variant.artifacts)) {
        const version = artifact.version;
        if (version.type !== "static") continue;
        const latest = version.releases.find(
          (release) => release.version === version.latest,
        );
        if (!latest?.checksum) {
          throw new Error(
            `${resourceId}/${variant.id}/${platform}: latest release ${version.latest} has no digest`,
          );
        }
      }
    }
  }
}

/**
 * Whether `out` already holds this manifest, apart from `updated_at`, together
 * with a snapshot that reproduces it and its summary. Files left incomplete by
 * an interrupted run are therefore rewritten.
 */
async function unchanged(
  out: string,
  template: ResourceManifestTemplate,
  manifest: ResourceManifest,
): Promise<boolean> {
  const previous = await readJson<ResourceManifest>(
    join(out, "resources.json"),
  );
  if (
    !previous?.updated_at ||
    !isDeepStrictEqual(
      { ...previous, updated_at: manifest.updated_at },
      manifest,
    )
  ) {
    return false;
  }
  if ((await readText(join(out, "summary.md"))) !== summarize(previous)) {
    return false;
  }
  const snapshot = await readJson<ManifestSnapshot>(join(out, "snapshot.json"));
  if (!snapshot) return false;
  try {
    return isDeepStrictEqual(
      compileManifest(template, snapshot, previous.updated_at),
      previous,
    );
  } catch (error) {
    if (error instanceof ManifestRuntimeError) return false;
    throw error;
  }
}

/** Writes a file in place through a temporary sibling and a rename. */
async function replace(path: string, text: string): Promise<void> {
  const temporary = `${path}.${process.pid}.tmp`;
  try {
    await writeFile(temporary, text);
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true });
  }
}

/** Generates the manifest; returns whether the output files were written. */
export async function generate(options: GenerateOptions): Promise<boolean> {
  const template = options.template ?? TEMPLATE;
  const now = (options.now ?? new Date()).toISOString();
  const context: Context = {
    api: options.api ?? GITHUB_API,
    http: { token: options.token, timeoutMs: options.timeoutMs },
    log: options.log ?? (() => {}),
  };

  const snapshot = await capture(template, now, context);
  const provisional = compileManifest(template, snapshot, now);
  const observed = await observe(template, provisional, context);

  // The observation must sit between two identical captures of each tag
  // release: an asset name without the version cannot reveal a re-publication.
  for (const [resourceId, sources] of Object.entries(snapshot.resources)) {
    const repo = repository(template, resourceId);
    for (const [sourceId, source] of Object.entries(sources)) {
      if (source.type !== "tag") continue;
      const again = await tagged(repo, source.release.tag, context);
      if (!isDeepStrictEqual(again, source.release)) {
        throw new Error(
          `${resourceId}.${sourceId}: release ${source.release.tag} changed while observing its version; upstream is publishing`,
        );
      }
      const version = observed.get(resourceId)?.get(sourceId);
      if (version !== undefined) source.version = version;
    }
  }
  const manifest = compileManifest(template, snapshot, now);
  requireLatestDigests(manifest);

  const out = resolve(options.out);
  if (await unchanged(out, template, manifest)) {
    context.log("unchanged; nothing written");
    return false;
  }
  // The snapshot goes first: a manifest is never newer than its snapshot, and
  // the summary is derived from the manifest.
  await mkdir(out, { recursive: true });
  await replace(
    join(out, "snapshot.json"),
    `${JSON.stringify(snapshot, null, 2)}\n`,
  );
  const manifestPath = join(out, "resources.json");
  await replace(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  await replace(join(out, "summary.md"), summarize(manifest));
  context.log(`wrote ${manifestPath}`);
  return true;
}

export async function main(argv = process.argv.slice(2)): Promise<void> {
  try {
    const { values } = parseArgs({
      args: argv,
      options: { out: { type: "string" } },
      strict: true,
    });
    if (!values.out) throw new Error("--out <directory> is required");
    await generate({
      out: values.out,
      token: process.env.GITHUB_TOKEN,
      log: (line) => console.log(line),
    });
  } catch (error) {
    const code =
      error instanceof ManifestRuntimeError ? error.code : "generate.failed";
    const message = error instanceof Error ? error.message : String(error);
    const line = (text: string) => text.replace(/[\r\n]+/g, " ");
    console.error(`error: ${code}: ${line(message)}`);
    // compile.failed and validate.failed list every underlying error.
    const errors =
      error instanceof ManifestRuntimeError &&
      Array.isArray(error.fields.errors)
        ? (error.fields.errors as {
            code: string;
            path: string;
            message: string;
          }[])
        : [];
    for (const item of errors) {
      console.error(`  ${item.code} at ${item.path}: ${line(item.message)}`);
    }
    process.exitCode = 1;
  }
}

if (import.meta.main) {
  await main();
}
