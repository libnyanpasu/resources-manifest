// What the viewer shows, computed from a manifest through the core: one row
// per artifact, filters, and the selection and plans a host would compute.
// Components only render these values.
import type {
  ResourceArtifactTemplate,
  ResourceManifest,
  SupportedPlatform,
} from "@manifest/metadata";
import {
  ManifestRuntimeError,
  artifactChecksumKind,
  planDownload,
  planVersion,
  selectVariant,
  validateManifest,
  type ChecksumKind,
  type DownloadPlanValue,
  type ValidatedManifest,
  type VariantSelection,
  type VersionPlanValue,
} from "@manifest/runtime/web";

/** A structured core error, or another failure as a message. */
export interface ErrorView {
  code: string;
  path: string;
  message: string;
}

export type Loaded =
  | { ok: true; validated: ValidatedManifest; manifest: ResourceManifest }
  | { ok: false; errors: ErrorView[] };

/** One artifact of one variant, as listed in the overview. */
export interface ArtifactRow {
  /** `<resource>/<variant>/<platform>`, unique within a manifest. */
  key: string;
  /** Dotted locator of the artifact, as in core error paths. */
  path: string;
  resourceId: string;
  resourceName: string;
  variantId: string;
  variantName: string;
  tags: string[];
  platform: SupportedPlatform;
  isDefault: boolean;
  artifact: string;
  versionType: "static" | "remote";
  /** Static latest version or remote fallback version. */
  version: string | null;
  releases: number;
  kind: ChecksumKind;
}

export interface Query {
  text: string;
  resource: string;
  platform: string;
  tag: string;
  kind: string;
}

export const EMPTY_QUERY: Query = {
  text: "",
  resource: "",
  platform: "",
  tag: "",
  kind: "",
};

export interface Facets {
  resources: { id: string; name: string }[];
  platforms: string[];
  tags: string[];
  kinds: string[];
}

/** A download plan through one mirror, or why the mirror cannot serve it. */
export interface DownloadView {
  version: string;
  /** `null` for the canonical URL. */
  mirror: string | null;
  plan: DownloadPlanValue | null;
  error: ErrorView | null;
}

/** A version plan through one mirror, or why the mirror cannot serve it. */
export interface VersionView {
  /** `null` for the canonical URLs. */
  mirror: string | null;
  plan: VersionPlanValue | null;
  error: ErrorView | null;
}

export interface ArtifactDetail {
  artifact: ResourceArtifactTemplate;
  /** The canonical plan first, then one per mirror in manifest order. */
  versions: VersionView[];
  downloads: DownloadView[];
}

export interface LoadTicket {
  /** Aborted once a newer load starts. */
  readonly signal: AbortSignal;
  /** Whether this is still the newest load, allowed to change the page. */
  isCurrent(): boolean;
}

/**
 * Starts loads in sequence: each new load aborts the previous one, and only the
 * newest may apply its result, so a slow request cannot replace or clear a
 * manifest loaded after it.
 */
export function loadSequence(): () => LoadTicket {
  let current: AbortController | null = null;
  return () => {
    current?.abort();
    const controller = new AbortController();
    current = controller;
    return {
      signal: controller.signal,
      isCurrent: () => current === controller,
    };
  };
}

export type Selection =
  | { ok: true; variantId: string; key: string }
  | { ok: false; error: ErrorView };

export function errorView(error: unknown): ErrorView {
  if (error instanceof ManifestRuntimeError) {
    return { code: error.code, path: error.path, message: error.message };
  }
  return {
    code: "viewer.error",
    path: "",
    message: error instanceof Error ? error.message : String(error),
  };
}

/** Decodes and validates manifest text; every validation error is listed. */
export function loadManifest(text: string): Loaded {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    return {
      ok: false,
      errors: [{ ...errorView(error), code: "viewer.invalid_json" }],
    };
  }

  try {
    const validated = validateManifest(raw);
    return { ok: true, validated, manifest: validated.value };
  } catch (error) {
    if (
      error instanceof ManifestRuntimeError &&
      Array.isArray(error.fields.errors)
    ) {
      return { ok: false, errors: error.fields.errors as ErrorView[] };
    }
    return { ok: false, errors: [errorView(error)] };
  }
}

export function artifactRows(manifest: ResourceManifest): ArtifactRow[] {
  const rows: ArtifactRow[] = [];
  for (const [resourceId, resource] of Object.entries(manifest.resources)) {
    for (const [index, variant] of resource.variants.entries()) {
      const entries = Object.entries(variant.artifacts) as [
        SupportedPlatform,
        ResourceArtifactTemplate,
      ][];
      for (const [platform, artifact] of entries) {
        const version = artifact.version;
        rows.push({
          key: `${resourceId}/${variant.id}/${platform}`,
          path: `resources.${resourceId}.variants[${index}].artifacts.${platform}`,
          resourceId,
          resourceName: resource.name,
          variantId: variant.id,
          variantName: variant.name,
          tags: variant.tags.map((tag) => tag.id),
          platform,
          isDefault: resource.default_variants[platform] === variant.id,
          artifact: artifact.artifact,
          versionType: version.type,
          version:
            version.type === "static"
              ? version.latest
              : (version.fallback?.version ?? null),
          releases: version.type === "static" ? version.releases.length : 0,
          kind: artifactChecksumKind(artifact),
        });
      }
    }
  }

  return rows;
}

export function facets(rows: ArtifactRow[]): Facets {
  const sorted = (values: Iterable<string>) => [...new Set(values)].sort();
  const resources = new Map(
    rows.map((row) => [row.resourceId, row.resourceName]),
  );

  return {
    resources: [...resources].map(([id, name]) => ({ id, name })),
    platforms: sorted(rows.map((row) => row.platform)),
    tags: sorted(rows.flatMap((row) => row.tags)),
    kinds: sorted(rows.map((row) => row.kind)),
  };
}

/**
 * Keeps rows matching every set filter. Text matches case-insensitively
 * against identifiers, names, tags, the artifact and the version.
 */
export function filterRows(rows: ArtifactRow[], query: Query): ArtifactRow[] {
  const words = query.text.toLowerCase().split(/\s+/).filter(Boolean);

  return rows.filter((row) => {
    if (query.resource && row.resourceId !== query.resource) return false;
    if (query.platform && row.platform !== query.platform) return false;
    if (query.tag && !row.tags.includes(query.tag)) return false;
    if (query.kind && row.kind !== query.kind) return false;
    const haystack = [
      row.resourceId,
      row.resourceName,
      row.variantId,
      row.variantName,
      row.platform,
      row.artifact,
      row.version ?? "",
      ...row.tags,
    ]
      .join(" ")
      .toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}

/** Runs `use` with a fresh handle and frees it afterwards. */
function withHandle<Handle extends { free(): void }, Result>(
  handle: Handle,
  use: (handle: Handle) => Result,
): Result {
  try {
    return use(handle);
  } finally {
    handle.free();
  }
}

/**
 * The version plans and download plans of one artifact, through the canonical
 * URLs and each mirror, as a host would request them. Downloads cover every
 * static release, or the remote fallback.
 */
export function artifactDetail(
  validated: ValidatedManifest,
  manifest: ResourceManifest,
  row: ArtifactRow,
): ArtifactDetail {
  return withHandle(
    selectVariant(validated, row.resourceId, row.platform, {
      id: row.variantId,
    }),
    (selected) => {
      const artifact = selected.value.artifacts[row.platform]!;
      const mirrors = [null, ...Object.keys(manifest.mirrors)];
      const versionPlans = mirrors.map((mirror): VersionView => {
        try {
          const plan = withHandle(
            planVersion(selected, { mirror }),
            (handle) => handle.value,
          );
          return { mirror, plan, error: null };
        } catch (error) {
          return { mirror, plan: null, error: errorView(error) };
        }
      });

      const versions =
        artifact.version.type === "static"
          ? artifact.version.releases.map((release) => release.version)
          : artifact.version.fallback
            ? [artifact.version.fallback.version]
            : [];
      const downloads = versions.flatMap((value) =>
        mirrors.map((mirror): DownloadView => {
          try {
            const plan = withHandle(
              planDownload(selected, { version: value, mirror }),
              (handle) => handle.value,
            );
            return { version: value, mirror, plan, error: null };
          } catch (error) {
            return {
              version: value,
              mirror,
              plan: null,
              error: errorView(error),
            };
          }
        }),
      );

      return { artifact, versions: versionPlans, downloads };
    },
  );
}

/** The artifact a host selecting with `selection` would get. */
export function simulate(
  validated: ValidatedManifest,
  resourceId: string,
  platform: SupportedPlatform,
  selection: VariantSelection,
): Selection {
  try {
    return withHandle(
      selectVariant(validated, resourceId, platform, selection),
      (selected) => ({
        ok: true,
        variantId: selected.value.id,
        key: `${resourceId}/${selected.value.id}/${platform}`,
      }),
    );
  } catch (error) {
    return { ok: false, error: errorView(error) };
  }
}
