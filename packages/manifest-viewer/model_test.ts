import assert from "node:assert/strict";
import { test } from "node:test";
import type { ResourceManifest } from "@manifest/metadata";
// The Node entry initializes the wasm glue that the browser entry shares.
import "@manifest/runtime";
import {
  EMPTY_QUERY,
  artifactDetail,
  artifactRows,
  facets,
  filterRows,
  loadManifest,
  loadSequence,
  simulate,
} from "./src/model.ts";

const digest = (byte: string) => `sha256:${byte.repeat(32)}` as const;

function manifest(): ResourceManifest {
  return {
    manifest_version: "2.0",
    updated_at: "2026-11-09T12:00:00Z",
    registries: {
      release: {
        name: "Releases",
        url_template: "https://origin.test/{release}/{artifact}",
      },
      meta: { name: "Metadata", url_template: "https://meta.test/{artifact}" },
    },
    mirrors: {
      proxy: {
        name: "Proxy",
        url_template: "https://proxy.test{path_and_query}",
        registries: ["release"],
        priority: 10,
      },
    },
    resources: {
      core: {
        name: "Core",
        registry: "release",
        tags: [
          { id: "channel:stable", name: "Stable", group: "channel" },
          { id: "channel:alpha", name: "Alpha", group: "channel" },
        ],
        default_variants: { "linux-amd64": "stable" },
        variants: [
          {
            id: "stable",
            tag_key: "channel:stable",
            name: "Stable",
            tags: [{ id: "channel:stable", name: "Stable", group: "channel" }],
            source: { release: "{version}" },
            artifacts: {
              "linux-amd64": {
                artifact: "core-{version}.gz",
                version: {
                  type: "static",
                  latest: "v2",
                  releases: [
                    { version: "v2", checksum: digest("22") },
                    { version: "v1", checksum: digest("11") },
                  ],
                },
              },
            },
          },
          {
            id: "alpha",
            tag_key: "channel:alpha",
            name: "Alpha",
            tags: [{ id: "channel:alpha", name: "Alpha", group: "channel" }],
            source: { release: "nightly" },
            artifacts: {
              "linux-amd64": {
                artifact: "core-{version}.gz",
                version: {
                  type: "remote",
                  fallback: { version: "alpha-1", checksum: digest("a1") },
                  latest: {
                    resource: { registry: "meta", artifact: "version.json" },
                    parser: { type: "json", pointer: "/version" },
                  },
                  checksum: {
                    resource: { registry: "meta", artifact: "sums.txt" },
                    parser: { type: "gnu", algorithm: "sha256" },
                  },
                },
              },
            },
          },
        ],
      },
    },
  };
}

function load() {
  const loaded = loadManifest(JSON.stringify(manifest()));
  assert.ok(loaded.ok);
  return loaded;
}

test("loading reports malformed JSON and every validation error", () => {
  const malformed = loadManifest("{");
  assert.equal(malformed.ok, false);
  if (malformed.ok) assert.fail();
  assert.equal(malformed.errors.length, 1);
  assert.equal(malformed.errors[0]!.code, "viewer.invalid_json");

  const invalid = manifest();
  invalid.resources.core!.registry = "missing";
  invalid.resources.core!.default_variants = { "linux-amd64": "nope" };
  const rejected = loadManifest(JSON.stringify(invalid));
  assert.equal(rejected.ok, false);
  if (rejected.ok) assert.fail();
  assert.deepEqual(
    rejected.errors.map((error) => [error.code, error.path]).sort(),
    [
      [
        "validate.default_variant",
        "resources.core.default_variants.linux-amd64",
      ],
      ["validate.reference", "resources.core.registry"],
    ],
  );

  const { validated, manifest: value } = load();
  assert.equal(validated.newerMinor, false);
  assert.deepEqual(value, manifest());
  validated.free();
});

test("rows list every artifact with its version and checksum kind", () => {
  const rows = artifactRows(manifest());
  assert.deepEqual(
    rows.map((row) => ({
      key: row.key,
      path: row.path,
      isDefault: row.isDefault,
      tags: row.tags,
      versionType: row.versionType,
      version: row.version,
      releases: row.releases,
      kind: row.kind,
    })),
    [
      {
        key: "core/stable/linux-amd64",
        path: "resources.core.variants[0].artifacts.linux-amd64",
        isDefault: true,
        tags: ["channel:stable"],
        versionType: "static",
        version: "v2",
        releases: 2,
        kind: "authoritative",
      },
      {
        key: "core/alpha/linux-amd64",
        path: "resources.core.variants[1].artifacts.linux-amd64",
        isDefault: false,
        tags: ["channel:alpha"],
        versionType: "remote",
        version: "alpha-1",
        releases: 0,
        kind: "parsed",
      },
    ],
  );
  assert.deepEqual(facets(rows), {
    resources: [{ id: "core", name: "Core" }],
    platforms: ["linux-amd64"],
    tags: ["channel:alpha", "channel:stable"],
    kinds: ["authoritative", "parsed"],
  });
});

test("filters combine, and every search word must match", () => {
  const rows = artifactRows(manifest());
  const keys = (query: Partial<typeof EMPTY_QUERY>) =>
    filterRows(rows, { ...EMPTY_QUERY, ...query }).map((row) => row.key);

  assert.equal(keys({}).length, 2);
  assert.deepEqual(keys({ text: "ALPHA core" }), ["core/alpha/linux-amd64"]);
  assert.deepEqual(keys({ text: "v2" }), ["core/stable/linux-amd64"]);
  assert.deepEqual(keys({ text: "alpha v2" }), []);
  assert.deepEqual(keys({ tag: "channel:stable" }), [
    "core/stable/linux-amd64",
  ]);
  assert.deepEqual(keys({ kind: "parsed" }), ["core/alpha/linux-amd64"]);
  assert.deepEqual(keys({ resource: "other" }), []);
  assert.deepEqual(keys({ platform: "darwin-arm64" }), []);
});

test("details plan every release through the canonical URL and each mirror", () => {
  const { validated, manifest: value } = load();
  const [stable, alpha] = artifactRows(value);

  const fixed = artifactDetail(validated, value, stable!);
  assert.deepEqual(
    fixed.versions.map((view) => [view.mirror, view.plan?.latest, view.error]),
    [
      [null, "v2", null],
      ["proxy", "v2", null],
    ],
  );
  assert.deepEqual(
    fixed.downloads.map((download) => [
      download.version,
      download.mirror,
      download.plan?.request.mirror_url ?? download.plan?.request.canonical_url,
      download.plan?.checksum?.kind,
    ]),
    [
      ["v2", null, "https://origin.test/v2/core-v2.gz", "authoritative"],
      ["v2", "proxy", "https://proxy.test/v2/core-v2.gz", "authoritative"],
      ["v1", null, "https://origin.test/v1/core-v1.gz", "authoritative"],
      ["v1", "proxy", "https://proxy.test/v1/core-v1.gz", "authoritative"],
    ],
  );

  // A host asking the proxy for the version is rejected: the proxy does not
  // serve the metadata registry of the version request.
  const remote = artifactDetail(validated, value, alpha!);
  assert.deepEqual(
    remote.versions.map((view) => [
      view.mirror,
      view.plan?.remote?.parts.map((part) => part.request.canonical_url) ??
        view.error?.code,
    ]),
    [
      [null, ["https://meta.test/version.json"]],
      ["proxy", "resolve.mirror_not_allowed"],
    ],
  );
  // The proxy serves only the release registry, so the metadata requests of
  // the remote checksum make it unusable.
  assert.deepEqual(
    remote.downloads.map((download) => [
      download.version,
      download.mirror,
      download.plan?.checksum_request?.request.canonical_url ??
        download.error?.code,
    ]),
    [
      ["alpha-1", null, "https://meta.test/sums.txt"],
      ["alpha-1", "proxy", "resolve.mirror_not_allowed"],
    ],
  );
  validated.free();
});

test("simulated selection follows defaults, tags and IDs like a host", () => {
  const { validated } = load();
  const pick = (selection: Parameters<typeof simulate>[3]) => {
    const result = simulate(validated, "core", "linux-amd64", selection);
    return result.ok ? result.key : result.error.code;
  };

  assert.equal(pick({}), "core/stable/linux-amd64");
  assert.equal(pick({ tags: ["channel:alpha"] }), "core/alpha/linux-amd64");
  assert.equal(pick({ id: "alpha" }), "core/alpha/linux-amd64");
  assert.equal(pick({ id: "beta" }), "resolve.variant_not_found");
  assert.equal(simulate(validated, "missing", "linux-amd64", {}).ok, false);
  validated.free();
});

test("a manifest without resources loads with no rows", () => {
  const empty = manifest();
  empty.resources = {};
  const loaded = loadManifest(JSON.stringify(empty));
  assert.ok(loaded.ok);
  assert.deepEqual(artifactRows(loaded.manifest), []);
  assert.deepEqual(facets([]), {
    resources: [],
    platforms: [],
    tags: [],
    kinds: [],
  });
  loaded.validated.free();
});

test("only the newest load is current, and older loads are aborted", () => {
  const start = loadSequence();
  const first = start();
  assert.ok(first.isCurrent());
  assert.equal(first.signal.aborted, false);

  const second = start();
  assert.equal(first.isCurrent(), false);
  assert.equal(first.signal.aborted, true);
  assert.ok(second.isCurrent());

  // Sequences are independent.
  assert.ok(loadSequence()().isCurrent());
  assert.ok(second.isCurrent());
});
