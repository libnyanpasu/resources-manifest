import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import templateSchema from "./schema/manifest-template.schema.json" with { type: "json" };
import manifestSchema from "./schema/manifest.schema.json" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateTemplate = ajv.compile(templateSchema);

test("template schema rejects unknown fields in template-only types", () => {
  const template = {
    manifest_version: 2,
    registries: {},
    resources: {
      example: {
        name: "Example",
        registry: "example",
        tags: { stable: { name: "Stable" } },
        variants: [{ name: "Stable", tags: ["stable"], artifacts: {} }],
      },
    },
  };
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
    assert.equal(
      validateTemplate.errors?.[0]?.keyword,
      "additionalProperties",
      location,
    );
    assert.equal(
      validateTemplate.errors?.[0]?.params.additionalProperty,
      "unknown_field",
      location,
    );
  }
});

test("regex version parser schema restricts group to u32 in both schemas", () => {
  for (const schema of [templateSchema, manifestSchema]) {
    const validateParser = ajv.compile(schema.$defs.RemoteVersionParser);
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
