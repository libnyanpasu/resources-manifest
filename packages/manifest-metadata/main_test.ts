import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import templateSchema from "./schema/manifest-template.schema.json" with { type: "json" };
import manifestSchema from "./schema/manifest.schema.json" with { type: "json" };
import referenceManifest from "../../crates/manifest-metadata/tests/fixtures/resources.json" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateTemplate = ajv.compile(templateSchema);
const validateManifest = ajv.compile(manifestSchema);

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
      variants: [
        {
          name: "Stable",
          tags: ["stable"],
          artifacts: {
            "windows-x86_64": {
              artifact: "example.zip",
              version: {
                type: "static",
                latest: "v1",
                releases: [{ version: "v1" }],
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
  assert.notEqual(manifestSchema.$id, templateSchema.$id);
  for (const schema of [manifestSchema, templateSchema]) {
    assert.ok(schema.$id.includes("/schema/2.0/"), schema.$id);
  }
});

test("both schemas reject unknown version parser types", () => {
  const compiled = structuredClone(referenceManifest);
  const authored = structuredClone(template);
  const remoteVersion =
    compiled.resources.clash_rs.variants[1].artifacts["windows-x86_64"].version;
  assert.ok(remoteVersion.version);
  Object.assign(
    authored.resources.example.variants[0].artifacts["windows-x86_64"],
    { version: remoteVersion },
  );
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

  for (const location of ["manifest", "resource", "variant", "tag"] as const) {
    const invalid = structuredClone(template);
    const nodes = {
      manifest: invalid,
      resource: invalid.resources.example,
      variant: invalid.resources.example.variants[0],
      tag: invalid.resources.example.tags.stable,
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
    const resource = extended.resources.clash_premium;
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
    "artifact",
    "version",
    "release",
  ] as const) {
    const extended = structuredClone(template);
    const artifact =
      extended.resources.example.variants[0].artifacts["windows-x86_64"];
    const nodes = {
      registry: extended.registries.example,
      mirror: extended.mirrors.direct,
      artifact,
      version: artifact.version,
      release: artifact.version.releases[0],
    };
    Object.assign(nodes[location], { unknown_field: true });
    assert.ok(
      validateTemplate(extended),
      `${location}: ${ajv.errorsText(validateTemplate.errors)}`,
    );
  }
});
