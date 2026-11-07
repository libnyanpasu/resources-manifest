import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { test } from "node:test";
import type { ResourceManifest } from "@manifest/metadata";
import {
  ManifestRuntimeError,
  artifactChecksumKind,
  checkPolicy,
  finishChecksum,
  parseChecksum,
  parseVersion,
  planDownload,
  planVersion,
  renderArtifact,
  renderTemplate,
  selectVariant,
  validateManifest,
  type LimitsInput,
} from "@manifest/runtime";

const digest = "0123456789abcdef".repeat(4);

function manifestInput(): ResourceManifest {
  return {
    manifest_version: "2.0",
    registries: {
      main: { name: "Main", url_template: "https://origin.test/{artifact}" },
    },
    mirrors: { proxy: { name: "Proxy", url_template: "{url}" } },
    resources: {
      sample: {
        name: "Sample",
        registry: "main",
        tags: [],
        default_variants: { "linux-amd64": "stable" },
        variants: [
          {
            id: "stable",
            name: "Stable",
            tag_key: "",
            tags: [],
            artifacts: {
              "linux-amd64": {
                artifact: "pkg-{version}.zip",
                version: {
                  type: "remote",
                  version: {
                    resource: { registry: "main", artifact: "version.txt" },
                    parser: { type: "text" },
                  },
                  checksum: {
                    resource: { registry: "main", artifact: "checksums.txt" },
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

function invalid(operation: () => unknown, path: string, exact = true): void {
  assert.throws(operation, (error: unknown) => {
    assert.ok(error instanceof ManifestRuntimeError);
    assert.equal(error.code, "wasm.invalid_input");
    assert.deepEqual(error.fields, {});
    if (exact) assert.equal(error.path, path);
    else assert.ok(error.path.startsWith(path), error.path);
    return true;
  });
}

test("deep host inputs fail without overflowing the wasm stack", () => {
  let deep: unknown = "leaf";
  for (let i = 0; i < 20_000; i++) deep = [deep];
  using manifest = validateManifest(manifestInput());
  using selected = selectVariant(manifest, "sample", "linux-amd64");
  using plan = planDownload(selected, { version: "v1" });
  const artifact =
    manifestInput().resources.sample.variants[0].artifacts["linux-amd64"]!;
  const cases: [string, () => unknown][] = [
    [
      "arg.manifest.extra",
      () => validateManifest({ ...manifestInput(), extra: deep }),
    ],
    [
      "arg.manifest.resources.sample.variants.0.artifacts.linux-amd64.version.extra",
      () => {
        const raw = manifestInput();
        Object.assign(
          raw.resources.sample.variants[0].artifacts["linux-amd64"]!.version,
          { extra: deep },
        );
        return validateManifest(raw);
      },
    ],
    [
      "arg.selection.tags",
      () =>
        selectVariant(manifest, "sample", "linux-amd64", {
          tags: deep as never,
        }),
    ],
    [
      "arg.options.source",
      () => planVersion(selected, { source: deep as never }),
    ],
    [
      "arg.options.source",
      () => planDownload(selected, { source: deep as never }),
    ],
    [
      "arg.options.source",
      () => renderArtifact(selected, { source: deep as never }),
    ],
    [
      "arg.limits.body_bytes",
      () => planVersion(selected, null, { body_bytes: deep as never }),
    ],
    [
      "response.body",
      () => parseVersion({ type: "text" }, { body: deep as never }),
    ],
    [
      "response.headers",
      () =>
        parseChecksum(
          { type: "checksum" },
          { body: "", headers: deep as never },
          "pkg",
        ),
    ],
    ["response.body", () => finishChecksum(plan, { body: deep as never })],
    [
      "arg.parser.extra",
      () =>
        parseVersion({ type: "text", extra: deep } as never, { body: "v1" }),
    ],
    [
      "arg.parser.extra",
      () =>
        parseChecksum(
          { type: "checksum", extra: deep } as never,
          { body: "" },
          "pkg",
        ),
    ],
    [
      "arg.artifact.extra",
      () => artifactChecksumKind({ ...artifact, extra: deep } as never),
    ],
    [
      "arg.values.key.literal",
      () =>
        renderTemplate("{key}", { key: { literal: deep as never } }, "test"),
    ],
    ["arg.policy.extra", () => checkPolicy("none", { extra: deep } as never)],
  ];
  for (const [path, operation] of cases) invalid(operation, path, false);
  const cyclic: Record<string, unknown> = {};
  cyclic.self = cyclic;
  invalid(
    () => validateManifest({ ...manifestInput(), extra: cyclic }),
    "arg.manifest.extra",
    false,
  );
});

test("non-finite host numbers are rejected at their input paths", () => {
  using manifest = validateManifest(manifestInput());
  using selected = selectVariant(manifest, "sample", "linux-amd64");
  using plan = planDownload(selected, { version: "v1" });
  const limitKeys: (keyof LimitsInput)[] = [
    "body_bytes",
    "header_value_bytes",
    "regex_pattern_bytes",
    "regex_size_bytes",
    "template_depth",
    "render_output_bytes",
  ];
  for (const number of [NaN, Infinity, -Infinity]) {
    const raw = manifestInput();
    raw.mirrors.proxy.priority = number;
    invalid(() => validateManifest(raw), "arg.manifest.mirrors.proxy.priority");
    const parser = { type: "regex", pattern: "(.*)", group: number } as const;
    invalid(() => parseVersion(parser, { body: "v1" }), "arg.parser.group");
    const artifact =
      manifestInput().resources.sample.variants[0].artifacts["linux-amd64"]!;
    assert.equal(artifact.version.type, "remote");
    if (artifact.version.type !== "remote") assert.fail();
    artifact.version.version.parser = parser;
    invalid(
      () => artifactChecksumKind(artifact),
      "arg.artifact.version.version.parser.group",
    );
    for (const key of limitKeys) {
      const limits = { [key]: number };
      const operations = [
        () => planVersion(selected, null, limits),
        () => planDownload(selected, { version: "v1" }, limits),
        () => renderArtifact(selected, { version: "v1" }, limits),
        () => renderTemplate("literal", {}, "test", limits),
        () => parseVersion({ type: "text" }, { body: "v1" }, limits),
        () =>
          parseChecksum(
            { type: "checksum" },
            { body: `sha256:${digest}` },
            "pkg",
            limits,
          ),
      ];
      for (const operation of operations)
        invalid(operation, `arg.limits.${key}`);
    }
    invalid(
      () => finishChecksum(plan, { body: number as never }),
      "response.body",
    );
    invalid(
      () =>
        parseVersion(
          { type: "text" },
          { body: "v1", headers: [["header", number as never]] },
        ),
      "response.headers.0.1",
    );
  }
});

test("input depth guard accepts 64 containers and rejects the next", () => {
  let extra: unknown = "leaf";
  for (let i = 0; i < 63; i++) extra = [extra];
  using manifest = validateManifest({ ...manifestInput(), extra });
  assert.deepEqual(manifest.value, manifestInput());
  invalid(
    () => validateManifest({ ...manifestInput(), extra: [extra] }),
    "arg.manifest.extra",
    false,
  );
  const raw = manifestInput();
  let buffered: unknown = "leaf";
  for (let i = 0; i < 56; i++) buffered = [buffered];
  Object.assign(
    raw.resources.sample.variants[0].artifacts["linux-amd64"]!.version,
    { extra: buffered },
  );
  using withBufferedEnum = validateManifest(raw);
  assert.deepEqual(withBufferedEnum.value, manifestInput());
});

test("arbitrary map keys round-trip as own properties", () => {
  const raw = manifestInput();
  const source = { ["__proto__"]: "source-value" };
  raw.resources.sample.source = source;
  Object.defineProperty(raw.resources, "__proto__", {
    value: structuredClone(raw.resources.sample),
    enumerable: true,
  });
  Object.defineProperty(raw.registries, "__proto__", {
    value: structuredClone(raw.registries.main),
    enumerable: true,
  });
  Object.defineProperty(raw.mirrors, "__proto__", {
    value: structuredClone(raw.mirrors.proxy),
    enumerable: true,
  });
  using manifest = validateManifest(raw);
  const value = manifest.value;
  assert.deepEqual(value, raw);
  for (const map of [
    value.resources,
    value.registries,
    value.mirrors,
    value.resources.sample.source!,
  ]) {
    assert.ok(Object.hasOwn(map, "__proto__"));
    assert.equal(Object.getPrototypeOf(map), Object.prototype);
  }
  using selected = selectVariant(manifest, "__proto__", "linux-amd64");
  using plan = planVersion(selected, { source });
  assert.equal(
    plan.value.remote?.request.canonical_url,
    "https://origin.test/version.txt",
  );
  value.resources.sample.name = "changed";
  assert.equal(manifest.value.resources.sample.name, "Sample");
  assert.equal(
    renderTemplate(
      "{__proto__}",
      { ["__proto__"]: { literal: "kept" } },
      "test",
    ),
    "kept",
  );
});

test("mirror priorities round-trip at the safe integer boundaries", () => {
  for (const priority of [
    -(2 ** 53 + 2),
    -(2 ** 53),
    -Number.MAX_SAFE_INTEGER,
    Number.MAX_SAFE_INTEGER,
    2 ** 53,
    2 ** 53 + 2,
  ]) {
    const raw = manifestInput();
    raw.mirrors.proxy.priority = priority;
    using manifest = validateManifest(raw);
    assert.equal(manifest.value.mirrors.proxy.priority, priority);
  }
});

test("all handle getters translate wasm error objects", () => {
  const wasm = createRequire(import.meta.url)(
    "./wasm/manifest_wasm.js",
  ) as Record<string, { prototype: object }>;
  using manifest = validateManifest(manifestInput());
  using selected = selectVariant(manifest, "sample", "linux-amd64");
  using version = planVersion(selected);
  using download = planDownload(selected, { version: "v1" });
  const cases: [string, string, () => unknown][] = [
    ["ValidatedManifest", "value", () => manifest.value],
    ["ValidatedManifest", "newerMinor", () => manifest.newerMinor],
    ["SelectedArtifact", "value", () => selected.value],
    ["VersionPlan", "value", () => version.value],
    ["DownloadPlan", "value", () => download.value],
  ];
  for (const [name, property, read] of cases) {
    const prototype = wasm[name].prototype;
    const descriptor = Object.getOwnPropertyDescriptor(prototype, property)!;
    const error = {
      code: "wasm.serialization_failed",
      path: "arg.result",
      message: "getter failed",
      fields: {},
    };
    Object.defineProperty(prototype, property, {
      ...descriptor,
      get() {
        throw error;
      },
    });
    try {
      assert.throws(read, (caught: unknown) => {
        assert.ok(caught instanceof ManifestRuntimeError);
        assert.equal(caught.code, error.code);
        assert.equal(caught.path, error.path);
        assert.equal(caught.message, error.message);
        assert.deepEqual(caught.fields, error.fields);
        return true;
      });
    } finally {
      Object.defineProperty(prototype, property, descriptor);
    }
  }
});

test("selection and checksum plans retain state after parents are freed", () => {
  const manifest = validateManifest(manifestInput());
  const selected = selectVariant(manifest, "sample", "linux-amd64");
  manifest.free();
  assert.equal(selected.value.id, "stable");
  using version = planVersion(selected);
  assert.equal(version.value.remote?.parser.type, "text");
  using plan = planDownload(selected, { version: "v1" });
  selected[Symbol.dispose]();
  assert.equal(plan.value.artifact, "pkg-v1.zip");
  assert.deepEqual(finishChecksum(plan, { body: `${digest}  pkg-v1.zip` }), {
    checksum: `sha256:${digest}`,
    kind: "parsed",
  });
});
