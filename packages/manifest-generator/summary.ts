// Renders summary.md, a Markdown overview of a compiled manifest for
// reviewers (design §9.8). It is derived from the manifest alone and is not
// part of the wire format.
import type {
  RemoteVersionResourceTemplate,
  ResourceArtifactTemplate,
  ResourceManifest,
  SupportedPlatform,
} from "@manifest/metadata";
import { artifactChecksumKind } from "@manifest/runtime";

type CompiledResource = ResourceManifest["resources"][string];

const HEADER = [
  "| Variant | Platform | Artifact | Version | Version source | Checksum | Kind |",
  "| --- | --- | --- | --- | --- | --- | --- |",
];

/**
 * Characters that could end a table cell or row, open or close a code span,
 * escape a neighbor, or start HTML. Manifest strings may contain any of them;
 * pointers, for example, may hold every character.
 */
const UNSAFE = /[|\\`\r\n<>&]/g;

/** Replaces unsafe characters with numeric character references. */
function entities(text: string): string {
  return text.replace(UNSAFE, (char) => `&#${char.charCodeAt(0)};`);
}

/** Literal text in a heading, paragraph or table cell. */
function plain(text: string): string {
  return entities(text);
}

/** Literal text as code; HTML keeps unsafe characters out of the table syntax. */
function code(text: string): string {
  return /[|\\`\r\n<>&]/.test(text)
    ? `<code>${entities(text)}</code>`
    : `\`${text}\``;
}

/** A remote request as `registry artifact`, without the parser details. */
function request(resource: RemoteVersionResourceTemplate["resource"]): string {
  return [resource.registry, resource.artifact]
    .filter((part) => part !== undefined)
    .map(code)
    .join(" ");
}

function versionSource(part: RemoteVersionResourceTemplate): string {
  const parser = part.parser;
  const rule = "pointer" in parser ? ` ${code(parser.pointer)}` : "";
  return `${request(part.resource)} → ${plain(parser.type)}${rule}`;
}

/** The version, version source and checksum cells of an artifact. */
function describe(artifact: ResourceArtifactTemplate): string[] {
  const version = artifact.version;
  if (version.type === "static") {
    const count = version.releases.length;
    return [
      `${code(version.latest)} (${count} ${count === 1 ? "release" : "releases"})`,
      "static history",
      "release digests",
    ];
  }
  const latest = version.latest;
  const source =
    "parts" in latest
      ? [
          ...Object.entries(latest.parts).map(
            ([name, part]) => `${plain(name)}: ${versionSource(part)}`,
          ),
          `format ${code(latest.format)}`,
        ].join("<br>")
      : versionSource(latest);
  const checksum = version.checksum
    ? `${request(version.checksum.resource)} → ${plain(version.checksum.parser.type)}`
    : "none";
  return [
    version.fallback ? `${code(version.fallback.version)} (fallback)` : "—",
    source,
    checksum,
  ];
}

function origin(resource: CompiledResource): string {
  const variables = Object.entries(resource.source ?? {}).map(
    ([key, value]) => `, ${plain(key)} ${code(value)}`,
  );
  return `Registry ${code(resource.registry)}${variables.join("")}.`;
}

/** Renders the summary of a compiled manifest. */
export function summarize(manifest: ResourceManifest): string {
  const lines = [
    "# Resource manifest summary",
    "",
    `Manifest ${plain(manifest.manifest_version)}, updated ${plain(manifest.updated_at ?? "—")}. Generated from \`resources.json\`; consumers do not read this file.`,
  ];
  for (const [resourceId, resource] of Object.entries(manifest.resources)) {
    lines.push("", `## ${plain(resource.name)} (${code(resourceId)})`, "");
    lines.push(origin(resource), "", ...HEADER);
    for (const variant of resource.variants) {
      for (const [platform, artifact] of Object.entries(variant.artifacts)) {
        const isDefault =
          resource.default_variants[platform as SupportedPlatform] ===
          variant.id;
        const row = [
          `${code(variant.id)}${isDefault ? " (default)" : ""}`,
          code(platform),
          code(artifact.artifact),
          ...describe(artifact),
          plain(artifactChecksumKind(artifact)),
        ];
        lines.push(`| ${row.join(" | ")} |`);
      }
    }
  }
  return `${lines.join("\n")}\n`;
}
