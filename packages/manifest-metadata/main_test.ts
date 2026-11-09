import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import templateSchema from "./schema/manifest-template.schema.json" with { type: "json" };
import manifestSchema from "./schema/manifest.schema.json" with { type: "json" };
import snapshotSchema from "./schema/manifest-snapshot.schema.json" with { type: "json" };
import referenceManifest from "../../crates/manifest-metadata/tests/fixtures/resources.json" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateTemplate = ajv.compile(templateSchema);
const validateManifest = ajv.compile(manifestSchema);
const validateSnapshot = ajv.compile(snapshotSchema);

const template = {
  manifest_version: "2.0",
  registries: {
    example: {
      name: "Example",
      url_template: "https://example.com/{artifact}",
    },
  },
  mirrors: { direct: { name: "Direct", url_template: "{url}" } },
  resources: {
    example: {
      name: "Example",
      registry: "example",
      tags: { stable: { name: "Stable" } },
      releases: {
        stable: { type: "history", limit: 8 },
        alpha: { type: "tag", tag: "alpha" },
      },
      variants: [
        {
          name: "Stable",
          tags: ["stable"],
          artifacts: {
            "windows-x86_64": {
              artifact: "example-{version}.zip",
              version: { type: "static", releases: "stable" },
            },
            "linux-amd64": {
              artifact: "example-{version}.gz",
              version: {
                type: "remote",
                releases: "alpha",
                version: {
                  resource: { registry: "example", artifact: "version.txt" },
                  parser: { type: "text" } as { type: string },
                },
              },
            },
          },
        },
      ],
    },
  },
};

for (const schema of [manifestSchema, templateSchema]) {
  test(`${schema.title} ManifestVersion accepts only canonical major.minor strings`, () => {
    const validateVersion = ajv.compile({
      $ref: "#/$defs/ManifestVersion",
      $defs: schema.$defs,
    });
    for (const version of ["2.0", "2.7", "2.10"]) {
      assert.ok(
        validateVersion(version),
        `${version}: ${ajv.errorsText(validateVersion.errors)}`,
      );
    }
    for (const version of ["2.07", "2.1.5", "2", "2.", "3.0", " 2.0", 2]) {
      assert.equal(validateVersion(version), false, JSON.stringify(version));
    }
  });
}

test("schema IDs are distinct and include the current version path", () => {
  const ids = [manifestSchema.$id, templateSchema.$id, snapshotSchema.$id];
  assert.equal(new Set(ids).size, ids.length);
  for (const schema of [manifestSchema, templateSchema, snapshotSchema]) {
    assert.ok(schema.$id.includes("/schema/2.0/"), schema.$id);
  }
});

test("both schemas reject unknown version parser types", () => {
  const compiled = structuredClone(referenceManifest);
  const authored = structuredClone(template);
  const remoteVersion =
    compiled.resources.clash_rs.variants[1].artifacts["windows-x86_64"].version;
  assert.ok(remoteVersion.version);
  authored.resources.example.variants[0].artifacts[
    "linux-amd64"
  ].version.version = remoteVersion.version;
  const cases = [
    { validate: validateManifest, fixture: compiled },
    { validate: validateTemplate, fixture: authored },
  ];
  for (const { validate, fixture } of cases) {
    assert.ok(validate(fixture), ajv.errorsText(validate.errors));
  }

  remoteVersion.version.parser.type = "future-parser";
  for (const { validate, fixture } of cases) {
    assert.equal(validate(fixture), false);
    assert.ok(
      validate.errors?.some((error) =>
        error.instancePath.endsWith("/version/version/parser/type"),
      ),
      ajv.errorsText(validate.errors),
    );
  }
});

test("template schema rejects unknown fields in template-only types", () => {
  assert.ok(
    validateTemplate(template),
    ajv.errorsText(validateTemplate.errors),
  );

  for (const location of [
    "manifest",
    "resource",
    "variant",
    "tag",
    "history",
    "tag_source",
    "artifact",
    "static",
    "remote",
  ] as const) {
    const invalid = structuredClone(template);
    const resource = invalid.resources.example;
    const artifacts = resource.variants[0].artifacts;
    const nodes = {
      manifest: invalid,
      resource,
      variant: resource.variants[0],
      tag: resource.tags.stable,
      history: resource.releases.stable,
      tag_source: resource.releases.alpha,
      artifact: artifacts["windows-x86_64"],
      static: artifacts["windows-x86_64"].version,
      remote: artifacts["linux-amd64"].version,
    };
    Object.assign(nodes[location], { unknown_field: true });
    assert.equal(validateTemplate(invalid), false, location);
    assert.ok(
      validateTemplate.errors?.some(
        (error) =>
          error.keyword === "additionalProperties" &&
          error.params.additionalProperty === "unknown_field",
      ),
      location,
    );
  }
});

test("regex version parser schema restricts group to u32 in both schemas", () => {
  for (const schema of [templateSchema, manifestSchema]) {
    const validateParser = ajv.compile({
      $ref: "#/$defs/RemoteVersionParser",
      $defs: schema.$defs,
    });
    assert.ok(validateParser({ type: "regex", pattern: "(.*)" }));
    for (const group of [0, 1, 4_294_967_295]) {
      assert.ok(
        validateParser({ type: "regex", pattern: "(.*)", group }),
        ajv.errorsText(validateParser.errors),
      );
    }
    for (const group of [-1, 0.5, 4_294_967_296]) {
      assert.equal(
        validateParser({ type: "regex", pattern: "(.*)", group }),
        false,
        `group ${group} must be rejected`,
      );
    }
  }
});

test("compiled manifest schema accepts unknown fields at every object level", () => {
  for (const location of [
    "manifest",
    "resource",
    "variant",
    "artifact",
    "registry",
    "mirror",
  ] as const) {
    const extended = structuredClone(referenceManifest);
    const resource = extended.resources.clash_rs;
    const variant = resource.variants[0];
    const nodes = {
      manifest: extended,
      resource,
      variant,
      artifact: variant.artifacts["windows-x86_64"],
      registry: extended.registries["github-release"],
      mirror: extended.mirrors.direct,
    };
    Object.assign(nodes[location], { unknown_field: true });
    assert.ok(
      validateManifest(extended),
      `${location}: ${ajv.errorsText(validateManifest.errors)}`,
    );
  }
});

test("template schema accepts unknown fields in shared types", () => {
  for (const location of [
    "registry",
    "mirror",
    "remote_version",
    "remote_resource",
  ] as const) {
    const extended = structuredClone(template);
    const remote =
      extended.resources.example.variants[0].artifacts["linux-amd64"].version;
    const nodes = {
      registry: extended.registries.example,
      mirror: extended.mirrors.direct,
      remote_version: remote.version,
      remote_resource: remote.version.resource,
    };
    Object.assign(nodes[location], { unknown_field: true });
    assert.ok(
      validateTemplate(extended),
      `${location}: ${ajv.errorsText(validateTemplate.errors)}`,
    );
  }
});

test("snapshot schema accepts captured releases and requires decimal string IDs", () => {
  const release = {
    id: "9007199254740993",
    tag: "v1",
    url: "https://api.github.com/repos/o/r/releases/1",
    assets: [
      {
        id: "301",
        name: "pkg-v1.zip",
        digest: `sha256:${"ab".repeat(32)}`,
        updated_at: "2026-11-09T00:00:00Z",
      },
    ],
  };
  const snapshot = {
    observed_at: "2026-11-09T00:00:00Z",
    resources: {
      sample: {
        stable: { type: "history", releases: [release] },
        alpha: { type: "tag", release },
      },
    },
  };
  assert.ok(
    validateSnapshot(snapshot),
    ajv.errorsText(validateSnapshot.errors),
  );
  for (const id of [1, "", "0x1"]) {
    const invalid = structuredClone(snapshot);
    (invalid.resources.sample.stable.releases[0] as { id: unknown }).id = id;
    assert.equal(validateSnapshot(invalid), false, JSON.stringify(id));
  }
  const unknown = structuredClone(snapshot);
  Object.assign(unknown.resources.sample.alpha, { unknown_field: true });
  assert.equal(validateSnapshot(unknown), false);
});
