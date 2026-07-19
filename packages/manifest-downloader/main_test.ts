import { assert, assertEquals } from "@std/assert";
import { Ajv2020 } from "ajv";
import manifestSchema from "@manifest/metadata/schema/manifest" with {
  type: "json",
};
import referenceManifest from "../../crates/manifest-metadata/tests/fixtures/resources.json" with {
  type: "json",
};

const ajv = new Ajv2020({ allErrors: true });
const validateManifest = ajv.compile(manifestSchema);

Deno.test("reference compiled manifest passes the manifest schema", () => {
  assert(
    validateManifest(referenceManifest),
    ajv.errorsText(validateManifest.errors),
  );
});

Deno.test("schema rejects an incompatible manifest_version", () => {
  const invalid = structuredClone(referenceManifest) as Record<string, unknown>;
  invalid.manifest_version = 3;
  assertEquals(validateManifest(invalid), false);
});

Deno.test("ResourceChecksum enforces <algorithm>:<lowercase hex>", () => {
  const checksum = manifestSchema.$defs.ResourceChecksum;
  const validateChecksum = ajv.compile(checksum);

  assert(validateChecksum(`sha256:${"a".repeat(64)}`));
  assert(validateChecksum(`sha512:${"0123456789abcdef".repeat(8)}`));
  assert(validateChecksum(`md5:${"0".repeat(32)}`));

  // Wrong length, unsupported algorithm, and non-normalized casing.
  assertEquals(validateChecksum("sha256:abcd"), false);
  assertEquals(validateChecksum(`sha1:${"a".repeat(40)}`), false);
  assertEquals(validateChecksum(`SHA256:${"a".repeat(64)}`), false);
  assertEquals(validateChecksum(`sha256:${"A".repeat(64)}`), false);
});

Deno.test("optional fields must be absent, never null", () => {
  const tag = manifestSchema.$defs.ResourceTag;
  const validateTag = ajv.compile(tag);

  assert(validateTag({ id: "channel:alpha", name: "Alpha" }));
  assert(validateTag({ id: "channel:alpha", name: "Alpha", group: "channel" }));
  assertEquals(
    validateTag({ id: "channel:alpha", name: "Alpha", group: null }),
    false,
  );
});
