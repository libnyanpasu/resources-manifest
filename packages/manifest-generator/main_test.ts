import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import templateSchema from "@manifest/metadata/schema/template" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateTemplate = ajv.compile(templateSchema);

test("template schema accepts an authored template with normalizable checksum", () => {
  const template = {
    manifest_version: 2,
    registries: {
      "github-release": {
        name: "GitHub Releases",
        url_template:
          "https://github.com/{owner}/{repo}/releases/download/{release}/{artifact}",
      },
    },
    resources: {
      mihomo: {
        name: "Mihomo",
        registry: "github-release",
        tags: { "channel:stable": { name: "Stable" } },
        variants: [
          {
            name: "Stable",
            tags: ["channel:stable"],
            artifacts: {
              "windows-x86_64": {
                artifact: "mihomo-windows-amd64-{version}.zip",
                version: {
                  type: "static",
                  latest: "v1.2.3",
                  releases: [
                    // Uppercase hex is accepted as compiler input; the
                    // compiler normalizes to lowercase on output.
                    { version: "v1.2.3", checksum: `sha256:${"A".repeat(64)}` },
                  ],
                },
              },
            },
          },
        ],
      },
    },
  };
  assert.ok(
    validateTemplate(template),
    ajv.errorsText(validateTemplate.errors),
  );
});

test("template checksum accepts any casing the compiler can normalize", () => {
  const checksum = templateSchema.$defs.ResourceChecksum;
  const validateChecksum = ajv.compile(checksum);

  assert.ok(validateChecksum(`sha256:${"a".repeat(64)}`));
  assert.ok(validateChecksum(`SHA256:${"A".repeat(64)}`));
  assert.ok(validateChecksum(`Sha256:${"Ab".repeat(32)}`));
  assert.ok(validateChecksum(`MD5:${"F".repeat(32)}`));

  // Still rejects malformed values.
  assert.equal(validateChecksum("sha256:abcd"), false);
  assert.equal(validateChecksum(`sha1:${"a".repeat(40)}`), false);
  assert.equal(validateChecksum(`sha256:${"g".repeat(64)}`), false);
});

test("template priority accepts fractions and rejects null", () => {
  const mirror = templateSchema.$defs.MirrorDefinition;
  const validateMirror = ajv.compile(mirror);

  assert.ok(validateMirror({ name: "m", url_template: "{url}", priority: 10 }));
  assert.ok(
    validateMirror({ name: "m", url_template: "{url}", priority: 0.5 }),
  );
  assert.ok(
    validateMirror({ name: "m", url_template: "{url}", priority: -3.25 }),
  );
  assert.equal(
    validateMirror({ name: "m", url_template: "{url}", priority: null }),
    false,
  );
});
