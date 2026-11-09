import assert from "node:assert/strict";
import { test } from "node:test";
import { Ajv2020 } from "ajv/dist/2020.js";
import templateSchema from "@manifest/metadata/schema/template" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
const validateTemplate = ajv.compile(templateSchema);

function authoredTemplate() {
  return {
    manifest_version: "2.0",
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
        source: { owner: "MetaCubeX", repo: "mihomo" },
        tags: { "channel:stable": { name: "Stable" } },
        releases: {
          stable: { type: "history", limit: 8 },
          alpha: { type: "tag", tag: "Prerelease-Alpha" },
        },
        variants: [
          {
            name: "Stable",
            tags: ["channel:stable"],
            artifacts: {
              "windows-x86_64": {
                artifact: "mihomo-windows-amd64-{version}.zip",
                version: { type: "static", releases: "stable" },
              },
              "linux-amd64": {
                artifact: "mihomo-linux-amd64-{version}.gz",
                version: {
                  type: "remote",
                  releases: "alpha",
                  latest: {
                    resource: {
                      registry: "github-release",
                      artifact: "version.txt",
                    },
                    parser: { type: "text" },
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

test("template schema accepts release sources and authored artifacts", () => {
  const template = authoredTemplate();
  assert.ok(
    validateTemplate(template),
    ajv.errorsText(validateTemplate.errors),
  );
});

test("template schema bounds history limits and rejects compiled versions", () => {
  for (const [limit, valid] of [
    [1, true],
    [50, true],
    [0, false],
    [51, false],
  ] as const) {
    const template = authoredTemplate();
    template.resources.mihomo.releases.stable.limit = limit;
    assert.equal(validateTemplate(template), valid, `limit ${limit}`);
  }
  const template = authoredTemplate();
  Object.assign(
    template.resources.mihomo.variants[0]!.artifacts["windows-x86_64"].version,
    { latest: "v1", releases: [{ version: "v1" }] },
  );
  assert.equal(validateTemplate(template), false);
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
