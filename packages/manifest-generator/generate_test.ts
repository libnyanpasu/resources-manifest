import assert from "node:assert/strict";
import {
  mkdtemp,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";
import type {
  ManifestSnapshot,
  ResourceManifest,
  ResourceManifestTemplate,
} from "@manifest/metadata";
import {
  compileManifest,
  planVersion,
  selectVariant,
  validateManifest,
} from "@manifest/runtime";
import { getJson } from "./http.ts";
import { generate } from "./main.ts";
import { TEMPLATE } from "./template.ts";

const NOW = new Date("2026-11-09T12:00:00Z");
const digest = (byte: string) => `sha256:${byte.repeat(32)}`;

interface Release {
  id: number;
  /** Decimal ID emitted instead of `id`, for IDs beyond 2^53. */
  apiId?: string;
  tag_name: string;
  draft?: boolean;
  prerelease?: boolean;
  assets: { id: number; name: string; digest?: string }[];
}

/** A local stand-in for the GitHub REST API and release downloads. */
async function upstream(t: TestContext) {
  const releases: Release[] = [];
  const alpha: Release = {
    id: 900,
    tag_name: "alpha",
    prerelease: true,
    assets: [
      { id: 901, name: "pkg-alpha-1-linux.gz", digest: digest("a1") },
      { id: 902, name: "pkg-alpha-1-win.zip", digest: digest("a2") },
    ],
  };
  const versions: Record<string, string> = {
    "linux-amd64": "alpha-1\n",
    "windows-x86_64": "alpha-1\n",
  };
  const requests: string[] = [];
  const authorizations: (string | undefined)[] = [];
  let alphaReads = 0;
  const state = {
    releases,
    alpha,
    versions,
    requests,
    authorizations,
    /** Called before the n-th (1-based) read of the alpha release. */
    onAlphaRead: (_read: number) => {},
  };

  // IDs are written as raw number tokens, so they can exceed 2^53.
  const rawId = (release: Release) => release.apiId ?? String(release.id);
  const toApi = (release: Release) => ({
    id: `#ID:${rawId(release)}`,
    tag_name: release.tag_name,
    url: `${base}/api/repos/o/r/releases/${rawId(release)}`,
    draft: release.draft ?? false,
    prerelease: release.prerelease ?? false,
    published_at: `2026-11-0${(release.id % 9) + 1}T00:00:00Z`,
  });
  const page = <T>(items: T[], url: URL) => {
    const size = Number(url.searchParams.get("per_page"));
    const index = Number(url.searchParams.get("page"));
    return items.slice((index - 1) * size, index * size);
  };

  const server = createServer((request, response) => {
    const url = new URL(request.url!, base);
    requests.push(url.pathname + url.search);
    authorizations.push(request.headers.authorization);
    const json = (value: unknown) => {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(value).replace(/"#ID:(\d+)"/g, "$1"));
    };
    if (url.pathname === "/api/repos/o/r/releases") {
      return json(page(releases.map(toApi), url));
    }
    if (url.pathname === "/api/repos/o/r/releases/tags/alpha") {
      state.onAlphaRead(++alphaReads);
      return json(toApi(alpha));
    }
    const assets = /^\/api\/repos\/o\/r\/releases\/(\d+)\/assets$/.exec(
      url.pathname,
    );
    if (assets) {
      const release = [alpha, ...releases].find(
        (item) => rawId(item) === assets[1],
      );
      return json(
        page(
          release!.assets.map((asset) => ({
            ...asset,
            id: `#ID:${asset.id}`,
            digest: asset.digest ?? null,
            updated_at: `2026-11-09T0${asset.id % 10}:00:00Z`,
          })),
          url,
        ),
      );
    }
    const version = /^\/o\/r\/download\/alpha\/version-(.+)\.txt$/.exec(
      url.pathname,
    );
    if (version && versions[version[1]!]) {
      response.writeHead(200);
      return response.end(versions[version[1]!]);
    }
    response.writeHead(404);
    response.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(
    () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
        server.closeAllConnections();
      }),
  );
  const address = server.address();
  assert.ok(address && typeof address === "object");
  const base = `http://127.0.0.1:${address.port}`;
  return { ...state, base, state };
}

function template(base: string, limit = 2): ResourceManifestTemplate {
  const artifacts = {
    "linux-amd64": "pkg-{version}-linux.gz",
    "windows-x86_64": "pkg-{version}-win.zip",
  };
  return {
    manifest_version: "2.0",
    registries: {
      release: {
        name: "Releases",
        url_template: `${base}/{owner}/{repo}/download/{release}/{artifact}`,
      },
    },
    resources: {
      sample: {
        name: "Sample",
        registry: "release",
        source: { owner: "o", repo: "r" },
        tags: {
          "channel:stable": { name: "Stable", group: "channel" },
          "channel:alpha": { name: "Alpha", group: "channel" },
        },
        default_variants: {
          "linux-amd64": "stable",
          "windows-x86_64": "stable",
        },
        releases: {
          stable: { type: "history", limit },
          alpha: { type: "tag", tag: "alpha" },
        },
        variants: [
          {
            id: "stable",
            name: "Stable",
            tags: ["channel:stable"],
            source: { release: "{version}" },
            artifacts: Object.fromEntries(
              Object.entries(artifacts).map(([platform, artifact]) => [
                platform,
                { artifact, version: { type: "static", releases: "stable" } },
              ]),
            ),
          },
          {
            id: "alpha",
            name: "Alpha",
            tags: ["channel:alpha"],
            source: { release: "alpha" },
            artifacts: Object.fromEntries(
              Object.entries(artifacts).map(([platform, artifact]) => [
                platform,
                {
                  artifact,
                  version: {
                    type: "remote",
                    releases: "alpha",
                    version: {
                      resource: {
                        registry: "release",
                        artifact: "version-{platform}.txt",
                      },
                      parser: { type: "text" },
                    },
                  },
                },
              ]),
            ),
          },
        ],
      },
    },
  };
}

/** 100 prereleases and drafts on the first page force a second page. */
function seed(releases: Release[]): void {
  for (let id = 1; id <= 100; id++) {
    releases.push({
      id: 1000 + id,
      tag_name: `pre-${id}`,
      prerelease: id % 2 === 0,
      draft: id % 2 === 1,
      assets: [],
    });
  }
  const many = Array.from({ length: 120 }, (_, index) => ({
    id: 3000 + index,
    name: `extra-${String(index).padStart(3, "0")}.txt`,
  }));
  releases.push(
    {
      id: 30,
      tag_name: "v3",
      assets: [
        { id: 301, name: "pkg-v3-linux.gz", digest: digest("31") },
        {
          id: 302,
          name: "pkg-v3-win.zip",
          digest: "SHA256:" + "3B".repeat(32),
        },
        ...many,
      ],
    },
    {
      id: 20,
      tag_name: "v2",
      assets: [{ id: 201, name: "pkg-v2-linux.gz", digest: digest("21") }],
    },
    {
      id: 10,
      tag_name: "v1",
      assets: [{ id: 101, name: "pkg-v1-linux.gz", digest: digest("11") }],
    },
  );
}

async function output(t: TestContext): Promise<string> {
  const directory = await mkdtemp(
    fileURLToPath(new URL(".generate-test-", import.meta.url)),
  );
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

async function written(directory: string) {
  return {
    manifest: JSON.parse(
      await readFile(join(directory, "resources.json"), "utf8"),
    ) as ResourceManifest,
    snapshot: JSON.parse(
      await readFile(join(directory, "snapshot.json"), "utf8"),
    ) as ManifestSnapshot,
  };
}

test("captures paginated history and observes alpha versions", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  const out = await output(t);
  const logs: string[] = [];

  assert.equal(
    await generate({
      out,
      template: template(server.base),
      api: `${server.base}/api`,
      now: NOW,
      log: (line) => logs.push(line),
    }),
    true,
  );
  const { manifest, snapshot } = await written(out);

  const history = snapshot.resources.sample!.stable!;
  assert.equal(history.type, "history");
  if (history.type !== "history") assert.fail();
  assert.deepEqual(
    history.releases.map((release) => release.tag),
    ["v3", "v2"],
  );
  assert.equal(history.releases[0]!.assets.length, 122);
  assert.equal(history.releases[0]!.id, "30");
  assert.equal(snapshot.observed_at, NOW.toISOString());

  const [stable, alpha] = manifest.resources.sample!.variants;
  assert.deepEqual(stable!.artifacts["windows-x86_64"]!.version, {
    type: "static",
    latest: "v3",
    releases: [
      {
        version: "v3",
        checksum: digest("3b"),
        published_at: "2026-11-04T00:00:00Z",
      },
    ],
  });
  assert.deepEqual(
    stable!.artifacts["linux-amd64"]!.version.type === "static" &&
      stable!.artifacts["linux-amd64"]!.version.releases.map(
        (release) => release.version,
      ),
    ["v3", "v2"],
  );
  const remote = alpha!.artifacts["linux-amd64"]!.version;
  assert.equal(remote.type, "remote");
  if (remote.type !== "remote") assert.fail();
  assert.deepEqual(remote.fallback, {
    version: "alpha-1",
    checksum: digest("a1"),
    published_at: "2026-11-09T01:00:00Z",
  });
  assert.equal(manifest.updated_at, NOW.toISOString());
  assert.ok(logs.includes("observe sample.alpha = alpha-1"), logs.join("\n"));
  // The first page held only drafts and prereleases.
  assert.ok(
    server.requests.includes("/api/repos/o/r/releases?per_page=100&page=2"),
  );
});

test("identical inputs produce identical files and an unchanged run writes nothing", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  const first = await output(t);
  const second = await output(t);
  const options = {
    template: template(server.base),
    api: `${server.base}/api`,
    now: NOW,
  };

  await generate({ ...options, out: first });
  await generate({ ...options, out: second });
  for (const file of ["resources.json", "snapshot.json"]) {
    assert.equal(
      await readFile(join(first, file), "utf8"),
      await readFile(join(second, file), "utf8"),
      file,
    );
  }

  const before = await readFile(join(first, "resources.json"), "utf8");
  assert.equal(
    await generate({
      ...options,
      out: first,
      now: new Date("2026-11-10T00:00:00Z"),
    }),
    false,
  );
  assert.equal(await readFile(join(first, "resources.json"), "utf8"), before);
});

test("artifacts of one source reporting different versions fail without writing", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  server.versions["windows-x86_64"] = "alpha-2\n";
  const out = await output(t);

  await assert.rejects(
    generate({
      out,
      template: template(server.base),
      api: `${server.base}/api`,
      now: NOW,
    }),
    /sample\.alpha: artifacts report versions alpha-1 and alpha-2/,
  );
  assert.deepEqual(await readdir(out), []);
});

test("a tag release changing while its version is observed fails without writing", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  server.state.onAlphaRead = (read) => {
    if (read === 2) server.alpha.assets[0]!.digest = digest("b1");
  };
  const out = await output(t);

  await assert.rejects(
    generate({
      out,
      template: template(server.base),
      api: `${server.base}/api`,
      now: NOW,
    }),
    /sample\.alpha: release alpha changed while observing its version/,
  );
  assert.deepEqual(await readdir(out), []);
});

test("unchanged runs keep both files; an incomplete pair is rewritten", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  const out = await output(t);
  const options = {
    out,
    template: template(server.base),
    api: `${server.base}/api`,
    now: NOW,
  };
  await generate(options);
  const files = ["resources.json", "snapshot.json"];
  const before = await Promise.all(
    files.map(async (file) => (await stat(join(out, file))).mtimeMs),
  );

  assert.equal(
    await generate({ ...options, now: new Date("2026-11-10T00:00:00Z") }),
    false,
  );
  assert.deepEqual(
    await Promise.all(
      files.map(async (file) => (await stat(join(out, file))).mtimeMs),
    ),
    before,
  );

  // A run interrupted after the manifest left no snapshot behind.
  await rm(join(out, "snapshot.json"));
  assert.equal(await generate(options), true);
  assert.deepEqual((await readdir(out)).sort(), files);

  // Earlier non-atomic writers could leave a truncated file of either kind.
  for (const file of files) {
    const text = await readFile(join(out, file), "utf8");
    await writeFile(join(out, file), text.slice(0, text.length / 2));
    assert.equal(await generate(options), true, file);
    assert.equal(await readFile(join(out, file), "utf8"), text, file);
  }

  // A valid snapshot that no longer reproduces the manifest is replaced too.
  const snapshotPath = join(out, "snapshot.json");
  const snapshot = JSON.parse(
    await readFile(snapshotPath, "utf8"),
  ) as ManifestSnapshot;
  const history = snapshot.resources.sample!.stable!;
  assert.equal(history.type, "history");
  if (history.type !== "history") assert.fail();
  history.releases[0]!.assets.find(
    (asset) => asset.name === "pkg-v3-linux.gz",
  )!.digest = `sha256:${"ff".repeat(32)}`;
  await writeFile(snapshotPath, JSON.stringify(snapshot));
  assert.equal(await generate(options), true);
  const { manifest, snapshot: repaired } = await written(out);
  assert.deepEqual(
    compileManifest(options.template, repaired, manifest.updated_at!),
    manifest,
  );
});

test("the token is sent only to the HTTPS GitHub API origin", async (t) => {
  const seen = new Map<string, string | null>();
  t.mock.method(globalThis, "fetch", async (url: string, init: RequestInit) => {
    seen.set(url, new Headers(init.headers).get("authorization"));
    return new Response("[]");
  });
  const urls = [
    "https://api.github.com/repos/o/r/releases",
    "http://api.github.com/repos/o/r/releases",
    "https://api.github.com:8443/repos/o/r/releases",
    "https://github.com/repos/o/r/releases",
  ];
  for (const url of urls) await getJson(url, { token: "secret" });
  assert.deepEqual(Object.fromEntries(seen), {
    [urls[0]!]: "Bearer secret",
    [urls[1]!]: null,
    [urls[2]!]: null,
    [urls[3]!]: null,
  });
});

test("GitHub IDs keep every digit and the token stays off other origins", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  server.releases.find((release) => release.tag_name === "v3")!.apiId =
    "9007199254740993";
  const out = await output(t);

  await generate({
    out,
    template: template(server.base),
    api: `${server.base}/api`,
    now: NOW,
    token: "secret",
  });
  const { snapshot } = await written(out);
  const history = snapshot.resources.sample!.stable!;
  assert.equal(history.type, "history");
  if (history.type !== "history") assert.fail();
  assert.equal(history.releases[0]!.id, "9007199254740993");
  assert.ok(
    server.requests.includes(
      "/api/repos/o/r/releases/9007199254740993/assets?per_page=100&page=1",
    ),
  );
  assert.deepEqual(new Set(server.authorizations), new Set([undefined]));
});

test("a latest stable release without a digest fails without writing", async (t) => {
  const server = await upstream(t);
  seed(server.releases);
  delete server.releases.find((release) => release.tag_name === "v3")!
    .assets[0]!.digest;
  const out = await output(t);

  await assert.rejects(
    generate({
      out,
      template: template(server.base),
      api: `${server.base}/api`,
      now: NOW,
    }),
    /sample\/stable\/linux-amd64: latest release v3 has no digest/,
  );
  assert.deepEqual(await readdir(out), []);
});

test("the production template compiles and plans every alpha version", () => {
  const snapshot: ManifestSnapshot = {
    observed_at: NOW.toISOString(),
    resources: {},
  };
  const version = {
    clash_rs: "0.10.11-alpha+sha.bd22442",
    meow: "alpha-3d2d196",
    mihomo: "alpha-478af2a",
  };
  for (const [resourceId, resource] of Object.entries(TEMPLATE.resources)) {
    const names = (tag: string) =>
      Object.values(resource.variants[0]!.artifacts).map((artifact, index) => ({
        id: String(index + 1),
        name: artifact.artifact.replaceAll("{version}", tag),
        digest: `sha256:${"ab".repeat(32)}` as const,
        updated_at: NOW.toISOString(),
      }));
    const release = (tag: string) => ({
      id: "1",
      tag,
      url: "https://api.github.com/repos/o/r/releases/1",
      assets: names(tag),
    });
    const alpha = version[resourceId as keyof typeof version];
    snapshot.resources[resourceId] = {
      stable: { type: "history", releases: [release("v1.0.0")] },
      alpha: { type: "tag", version: alpha, release: release(alpha) },
    };
  }

  const manifest = compileManifest(TEMPLATE, snapshot, NOW.toISOString());
  using validated = validateManifest(manifest);
  const plans = Object.keys(TEMPLATE.resources).map((resourceId) => {
    using selected = selectVariant(validated, resourceId, "linux-aarch64", {
      tags: ["channel:alpha"],
    });
    using plan = planVersion(selected);
    return plan.value.remote!.parts.map((part) => [
      part.name,
      part.request.canonical_url,
    ]);
  });
  assert.deepEqual(plans, [
    [
      [
        "commit",
        "https://api.github.com/repos/ibigbug/clash-rs/commits/latest",
      ],
      [
        "stable",
        "https://api.github.com/repos/ibigbug/clash-rs/releases/latest",
      ],
    ],
    [
      [
        null,
        "https://github.com/meow-rs/meow-rs/releases/expanded_assets/Prerelease-Alpha",
      ],
    ],
    [
      [
        null,
        "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
      ],
    ],
  ]);
});
