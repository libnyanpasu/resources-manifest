import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import manifestSchema from "@manifest/metadata/schema/manifest" with { type: "json" };
import referenceManifest from "../../crates/manifest-metadata/tests/fixtures/resources.json" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateManifest = ajv.compile(manifestSchema);

test("reference compiled manifest passes the manifest schema", () => {
  assert.ok(
    validateManifest(referenceManifest),
    ajv.errorsText(validateManifest.errors),
  );
});

test("schema rejects an incompatible manifest_version", () => {
  const invalid = structuredClone(referenceManifest) as Record<string, unknown>;
  invalid.manifest_version = "3.0";
  assert.equal(validateManifest(invalid), false);
});

test("ResourceChecksum enforces <algorithm>:<lowercase hex>", () => {
  const checksum = manifestSchema.$defs.ResourceChecksum;
  const validateChecksum = ajv.compile(checksum);

  assert.ok(validateChecksum(`sha256:${"a".repeat(64)}`));
  assert.ok(validateChecksum(`sha512:${"0123456789abcdef".repeat(8)}`));
  assert.ok(validateChecksum(`md5:${"0".repeat(32)}`));

  // Wrong length, unsupported algorithm, and non-normalized casing.
  assert.equal(validateChecksum("sha256:abcd"), false);
  assert.equal(validateChecksum(`sha1:${"a".repeat(40)}`), false);
  assert.equal(validateChecksum(`SHA256:${"a".repeat(64)}`), false);
  assert.equal(validateChecksum(`sha256:${"A".repeat(64)}`), false);
});

test("optional fields must be absent, never null", () => {
  const tag = manifestSchema.$defs.ResourceTag;
  const validateTag = ajv.compile(tag);

  assert.ok(validateTag({ id: "channel:alpha", name: "Alpha" }));
  assert.ok(
    validateTag({ id: "channel:alpha", name: "Alpha", group: "channel" }),
  );
  assert.equal(
    validateTag({ id: "channel:alpha", name: "Alpha", group: null }),
    false,
  );
});
