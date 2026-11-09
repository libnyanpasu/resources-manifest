# Resource manifest v2

This document describes the variant-aware resource manifest generated at
`manifest/resources.json`. The existing `manifest/version.json` remains in place
while updater consumers migrate.

## Design goals

1. A resource key identifies a **core family**, not a release channel. Stable,
   alpha and future channels are variants of the same resource and are selected
   through tags.
2. A variant is identified semantically by its **exact tag combination**.
   `variant.id` is only a stable, human-readable alias.
3. Every compiled variant contains expanded tag objects, so a UI can render the
   channel, CPU level and toolchain without joining another catalog.
4. Every artifact declares how its version is resolved:
   - `static`: the manifest contains a bounded, traceable release history;
   - `remote`: the manifest tells the backend which URLs to request and how to
     parse the latest version and checksum.
5. Checksums are serialized as `<algorithm>:<hex>`, for example
   `sha512:0123...`.
6. Canonical artifact/metadata location (`registry`) is separate from URL
   acceleration or alternative delivery (`mirror`). The same mirror pipeline is
   used for downloads, remote version files and remote checksum metadata.
7. Registries and mirrors are named templates and may also be injected at
   runtime for user-defined download sources.

## Versioning

`manifest_version` is a `"<major>.<minor>"` string:

```json
{
  "manifest_version": "2.0"
}
```

The current major is 2. Rust represents the version as
`ManifestVersion { minor: u32 }`, with a fixed major; the generated TypeScript
type is `` `2.${number}` `` and the JSON Schema pattern is
`^2\.(0|[1-9][0-9]*)$`. Producers write `CURRENT_MINOR` (currently 0).
The minor has no leading zeros: `"2.07"` fails with
`manifest.invalid_version_format`.

Consumers reject a non-string or malformed version with
`manifest.invalid_version_format`, and a different major with
`manifest.unsupported_major`. They accept any minor within major 2, reporting
`newer_minor: true` when the minor exceeds the consumer's `CURRENT_MINOR`.

After the first release, compatible changes increment only the minor:

- new optional fields are ignored by older consumers;
- new platform keys are skipped by older consumers in compiled artifacts and
  platform defaults;
- new version or checksum parser types decode as unsupported. Only resolving
  the affected artifact fails with `resolve.unsupported_parser`; other artifacts
  remain usable. Decoding preserves the original unknown `type` tag as the
  error field `parser_type`. Older consumers derive `none` for artifacts with
  unsupported parsers, which no verification policy accepts, so the artifact is
  unusable rather than silently unverified.

Changes that require a new major are:

- adding required fields;
- changing the semantics of an existing field or parser;
- removing fields;
- switching an existing artifact to a parser type unknown to older consumers,
  which removes an existing capability from those consumers.

A major upgrade follows the dual-file transition described in
[Migration from manifest v1](#migration-from-manifest-v1).

Permissive decoding applies only to compiled manifests. Templates are strict
(`deny_unknown_fields`) to catch authoring mistakes. Generated TypeScript and
JSON Schema describe the exact current-minor contract without unknown variants;
each Schema `$id` includes `major.minor`. A consumer must not reject a newer
minor solely because validation against an older Schema fails; consumer
validation uses `validate_manifest`.

Before the first release of any `@manifest/*` package or crate, the version stays
at `"2.0"`. All contract changes before that release are included in 2.0 without
incrementing the minor. The minor and major rules above apply after release.

## Resource identity and channel variants

The v1 manifest exposes separate keys such as `mihomo` and `mihomo_alpha`. The
v2 resource manifest merges them:

```json
{
  "resources": {
    "mihomo": {},
    "clash_rs": {}
  }
}
```

A Mihomo stable variant and alpha variant differ by their tag combination and
channel-specific source variables:

```ts
{
  id: "stable-goamd64-v2",
  name: "Stable · GOAMD64 v2",
  tags: [
    "channel:stable",
    "toolchain:go-current",
    "cpu:goamd64-v2",
  ],
  source: { release: "{version}" },
  artifacts: { /* ... */ },
}

{
  id: "alpha-goamd64-v2",
  name: "Alpha · GOAMD64 v2",
  tags: [
    "channel:alpha",
    "toolchain:go-current",
    "cpu:goamd64-v2",
  ],
  source: { release: "Prerelease-Alpha" },
  artifacts: { /* ... */ },
}
```

The compiler expands each tag and adds an order-independent `tag_key`:

```json
{
  "id": "alpha-goamd64-v2",
  "tag_key": "channel:alpha+cpu:goamd64-v2+toolchain:go-current",
  "name": "Alpha · GOAMD64 v2",
  "tags": [
    { "id": "channel:alpha", "name": "Alpha", "group": "channel" },
    {
      "id": "toolchain:go-current",
      "name": "Current Go",
      "group": "toolchain"
    },
    { "id": "cpu:goamd64-v2", "name": "GOAMD64 v2", "group": "cpu" }
  ]
}
```

A UI should render `variant.tags` directly. Selecting by exact tags remains
order-independent. Tag IDs in `tag_key` are sorted by Unicode code point, not by
locale. The UI uses `tag.id` and the resource key as translation keys, with
`name` only as a fallback.

Tag IDs match `^[A-Za-z0-9][A-Za-z0-9._:-]*$`; variant IDs match
`^[A-Za-z0-9][A-Za-z0-9._:+-]*$`. IDs are case-sensitive and are never trimmed.
The `+` separator is reserved in tag IDs. Catalog tag IDs are unique per resource,
variant tag IDs are unique within each variant, and variant IDs are unique per
resource. A canonical `tag_key` joins sorted tag IDs with `+`; an empty tag list
has key `""`. Exact combinations compare sets independently of the supplied key.
Only absent or empty (`""`) groups are exempt from mutual exclusion; whitespace
is a non-empty group.

## Static version history

A stable artifact can carry several releases and their artifact-specific
checksums:

```json
{
  "artifact": "mihomo-windows-amd64-v2-{version}.zip",
  "version": {
    "type": "static",
    "latest": "v1.19.28",
    "releases": [
      {
        "version": "v1.19.28",
        "checksum": "sha256:0123456789abcdef...",
        "published_at": "2026-07-08T00:34:23Z"
      },
      {
        "version": "v1.19.27",
        "checksum": "sha256:fedcba9876543210...",
        "published_at": "2026-06-06T07:56:33Z"
      }
    ]
  }
}
```

The current generator requests a bounded set of non-prerelease GitHub releases,
paginates both releases and their assets, and copies GitHub's asset `digest`
into the static history. The default trace depth is eight releases and can be
set from 1 to 50 with `NYANPASU_RESOURCE_HISTORY_LIMIT`. `GITHUB_TOKEN` is used
when available. An asset may still appear without `checksum` when the upstream
registry does not expose a digest.

The resolver only accepts a requested static version when that version is
present in the artifact's `releases` list. This prevents callers from rendering
an arbitrary historical URL and then accidentally treating an unrelated digest
as authoritative.

Checksums use a single serialized representation:

```text
md5:<32 hex characters>
sha256:<64 hex characters>
sha512:<128 hex characters>
```

The compiler normalizes the algorithm and hexadecimal value to lowercase.

## Remote version and checksum resources

Mutable prerelease tags often retain only the newest assets and cannot provide a
stable historical chain. Such an artifact uses a declarative remote resolution
plan instead of pretending to have history. This section describes the
implemented single-response form; a planned extension composes the remote
version from several responses (see
[the core design §9.2](../spec/2026-11-06-manifest-core-parsers/design.md#92-d4-clash-rust-alpha-的版本来源o-4)).
A single-response plan looks like this:

```json
{
  "artifact": "mihomo-windows-amd64-v2-{version}.zip",
  "version": {
    "type": "remote",
    "fallback": {
      "version": "alpha-e911985"
    },
    "latest": {
      "resource": {
        "registry": "github-release",
        "artifact": "version.txt"
      },
      "parser": {
        "type": "text",
        "trim": true
      }
    },
    "checksum": {
      "resource": {
        "registry": "github-release-metadata"
      },
      "parser": {
        "type": "github-release-assets"
      }
    }
  }
}
```

For the Mihomo alpha variant, variant-level `source.release` is
`Prerelease-Alpha`. The two remote requests therefore resolve to:

```text
https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt
https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha
```

The first response supplies the latest mutable version. The second response
contains GitHub's artifact digest; the parser selects the digest from the asset
entry whose name exactly matches the rendered artifact name.

`fallback` is the snapshot observed when the manifest was generated. When the
GitHub release API exposes the current asset digest, the generator also embeds
that artifact-specific checksum in the fallback. It is still not authoritative
latest state. The separate reference-result fields `fallback_checksum` and
`checksum_remote` become kind-bearing checksum results and request plans:
`fallback.checksum` is a `snapshot`, usable only under an explicit offline policy
and only when the resolved version exactly equals `fallback.version`; the remote
checksum resource and its parser belong to the download plan, and parsing that
response produces a checksum with kind `parsed`. A remote download plan requires
an explicit version, so a consumer cannot silently skip the remote version
request.

All parsers take a `RemoteResponse { body: &str, headers: &[(String, String)] }`.
Header names are case-insensitive. A body exceeding the limit in
[Limits](#limits) fails with `parser.body_too_large`. Body parsers do not read
headers; only an explicitly declared `header` checksum parser does so.

### Composite remote versions

Some upstreams publish no version file. A remote `latest` may instead combine
several responses: each named part is an ordinary remote version request with
its own parser, and `format` joins the part results. Clash Rust alpha combines
the latest stable tag with the commit of its rolling `latest` tag:

```json
{
  "parts": {
    "stable": {
      "resource": { "registry": "github-api", "artifact": "releases/latest" },
      "parser": {
        "type": "json",
        "pointer": "/tag_name",
        "pattern": "^v?([0-9].*)$"
      }
    },
    "commit": {
      "resource": { "registry": "github-api", "artifact": "commits/{release}" },
      "parser": {
        "type": "json",
        "pointer": "/sha",
        "pattern": "^([0-9a-f]{7})[0-9a-f]{33}$"
      }
    }
  },
  "format": "{stable}-alpha+sha.{commit}"
}
```

An object with a `format` key is a composite version; otherwise it is a single
`{ resource, parser }`. A composite version has one to four parts, named
`^[A-Za-z0-9_]+$` and not after a built-in variable, and `format` must reference
every part and nothing else (`validate.version_parts`,
`validate.template_variable`). Part results are inserted literally; the combined
value must be a `VersionLabel`, otherwise `parser.invalid_version` at
`parser.format`. Any failing part fails the whole version.

A version plan lists one request per part, in code-point order of part names; a
single version is one unnamed part. Hosts perform every request and pass the
responses, in plan order, to `finish_version`. A wrong number of responses fails
with `resolve.version_responses` before parsing, and a part's parser error is
reported at `responses[<i>].body`. Mirrors and checksum kinds work as for a
single version.

### Supported remote version parsers

- `text`: `trim` defaults to `true`, removing leading and trailing whitespace
  before validation. With `trim: false`, validate the body unchanged. The result
  must satisfy `VersionLabel`; for example, an unchanged trailing newline is
  invalid.
- `regex`: use the Rust `regex` crate dialect, with no external flags; inline
  flags such as `(?i)` are allowed. Take the configured capture `group` from the
  first match, trim its leading and trailing whitespace, then validate it as a
  `VersionLabel`. `group` defaults to 1; 0 selects the entire match. Validation
  compiles the pattern, enforces the compilation size limit, and checks that
  `group` is less than the capture count, including group 0.
- `json`: `{ pointer, pattern?, group? }`. Decode the body as JSON; a body that
  is not valid JSON or repeats a key in any object fails with
  `parser.invalid_json`. `pointer` is an RFC 6901 JSON Pointer: empty, or
  starting with `/` with every `~` followed by `0` or `1`; otherwise
  compilation fails with `parser.invalid_pointer`. Array indices follow RFC 6901,
  so leading zeros and `-` select nothing. A missing value or one that is not a
  string fails with `parser.not_found`. Without `pattern` the whole string is
  the result and `group` is unused; with `pattern`, the string is matched
  exactly like the `regex` parser matches a body, including its limits and
  `group` rules. The result is trimmed and validated as a `VersionLabel`.

Version parsers do not read headers. A future version-header parser would be a
minor addition after release.

### Supported remote checksum parsers

- `checksum`: trim leading and trailing whitespace from the body, then parse
  the complete result as a `ResourceChecksum` (`<algorithm>:<hex>`).
- `gnu`: split lines on `\n` or `\r\n`. Each line has the format
  `<hex>[ \t]+[* ]?<name>[ \t]*`. The name must exactly equal the artifact name,
  and the hex length must match the configured algorithm. Duplicate matching
  lines with identical digests collapse to one result; conflicting digests fail
  with `parser.ambiguous`. No matching name fails with `parser.not_found`.
- `bsd`: parse lines of the form `<LABEL> (<name>) = <hex>`. Compare `LABEL`
  case-insensitively with the configured algorithm and skip lines with a
  different label. The artifact name must match exactly. Conflicting matching
  digests fail with `parser.ambiguous`.
- `github-release-assets`: split the HTML into asset entries. Take each name
  from the final path segment of its download link's `href`, percent-decoded,
  and read a digest only from that same entry. Names must match exactly, so
  `selected.zip` does not match `prefix-selected.zip`. No matching entry fails
  with `parser.not_found`; a matching entry without a digest fails with
  `parser.digest_missing`; conflicting digests in entries with the same name
  fail with `parser.ambiguous`. Never search a nearby character window or borrow
  an adjacent entry's digest.
- `header`: declare
  `{ type: "header", name, algorithm, encoding: "hex" | "base64" }`. `name` must
  be a non-empty valid HTTP header token. Read only that response header,
  comparing names case-insensitively. A missing header fails with
  `parser.not_found`; repeated headers with different values fail with
  `parser.ambiguous`. With `hex`, parse the hex value; with `base64`, decode the
  value and convert the bytes to lowercase hex. The decoded digest length must
  match the configured algorithm. Invalid base64 fails with
  `parser.invalid_encoding`; non-hex characters, odd hex length, or a decoded
  digest length that differs from the algorithm fail with
  `parser.invalid_checksum`. A single header value exceeding the single-header-value limit (default
  8 KiB, see [Limits](#limits)) fails first
  with `parser.body_too_large`, carrying `{ limit, actual }`; this code applies
  to both the body and a single header value.

These parser names are protocol between the manifest generator and backend.
Rust consumers depend on `manifest-core`, which uses `manifest-parser` for the
shared parser implementation.

## Checksum kinds

The trust anchor is a signed manifest. The signature covers only the manifest
itself. The signing scheme is minisign; the signing interface and workflow are
defined in [the core design §9.1](../spec/2026-11-06-manifest-core-parsers/design.md#91-d5-签名接口o-2).
Only checksums embedded in static release history are `authoritative` for
normal resolution.

`ChecksumKind` wire names are fixed as lowercase snake_case: `authoritative`,
`parsed`, `snapshot`, `none` (`#[serde(rename_all = "snake_case")]` in Rust).

| `ChecksumKind`  | Source                                                                                                     | Default policy                     |
| --------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| `authoritative` | A static release's embedded `checksum`, covered by the manifest signature                                  | Accept                             |
| `parsed`        | A remote response parsed by a declared checksum rule, whether body or header, through canonical or mirror  | Reject; requires explicit opt-in   |
| `snapshot`      | A remote artifact's embedded `fallback.checksum`, only when the resolved version equals `fallback.version` | Reject; requires an offline opt-in |
| `none`          | No checksum                                                                                                | Always reject; no opt-in           |

The following presentation is recommended; specific UI wording is out of scope
(design spec §12). These recommendations do not replace the policy decision.

| Kind            | Recommended presentation                                                                                            |
| --------------- | ------------------------------------------------------------------------------------------------------------------- |
| `authoritative` | No warning                                                                                                          |
| `parsed`        | Warn that integrity is anchored on a value fetched at update time, rather than the signed manifest                  |
| `snapshot`      | Warn that this is an embedded offline snapshot, applicable only when the resolved version equals `fallback.version` |
| `none`          | Hard block; no verification policy can accept it                                                                    |

The host must expose the effective policy decision alongside the kind: accept,
or reject with a reason, as returned by `check_policy` under the host's
`VerifyPolicy`. Kind alone does not determine the outcome.

`artifact_checksum_kind(&ResourceArtifactTemplate) -> ChecksumKind` derives the
normal resolution kind without network access:

- static release with a checksum: `authoritative`;
- static release without a checksum: `none`;
- remote artifact with a declared checksum rule: `parsed`;
- remote artifact without a checksum rule: `none`.

Artifacts with an unsupported version or checksum parser derive `none`.
A batch helper lists the kind of every variant of a resource on a platform
(design spec §7.3), so the host can expose per-variant kinds before selection.

An embedded fallback does not change this static classification. `snapshot`
applies only when the host explicitly chooses that fallback under its offline
policy and the versions match. Static derivation serves pre-selection display;
plans and resolved results carry the applicable kind at execution time, alongside
the effective host policy decision. The UI receives per-variant kinds and policy
decisions through the Rust backend before the user selects or updates a core,
including alpha variants.

Only a declared parser rule can produce `parsed`. Hosts must not infer
checksums from undeclared `Digest`, `Repr-Digest`, `ETag`, `x-*-checksum` or any
other response header or source. Reading a header requires a `header` parser in
the manifest. A canonical transport does not promote a parsed checksum to
`authoritative`; a mirror does not change kind either. The actual transport
channel is returned to core only for diagnostics and logging.

The default host policy accepts only `authoritative`. `--allow-parsed` and
`--allow-snapshot` explicitly enable the corresponding kind and emit a warning;
`none` always fails. An md5 checksum does not change kind, but a host may also
reject algorithms independently. A digest mismatch always stops the operation
before decompression or replacement.

## Backend resolution flow

For a resource and platform:

1. Run `validate_manifest`, derive per-variant kinds, and expose them alongside
   effective host policy decisions before selection or update.
2. Select the variant by `variant.id`, exact tags, or the platform default.
3. Obtain a version plan from `plan_version`. For a static artifact, choose
   `latest` or another entry from `releases`; a version outside that history is
   rejected.
4. For a remote artifact, request the planned canonical or mirror version URL
   and pass the response body and headers to its version parser. Pass the
   resulting version explicitly to `plan_download`.
5. Render the concrete artifact name and URL. The download plan carries
   `canonical_url`, optional `mirror_url`, its compiled parser where applicable,
   and `ChecksumKind`.
6. Use the embedded checksum for a static release (`authoritative`). For a
   remote artifact, fetch its declared checksum resource and pass the plan and
   `RemoteResponse` to `finish_checksum`, yielding `parsed` for the concrete
   artifact named by the plan. An explicitly selected offline fallback checksum
   is `snapshot` only if its version matches exactly. Missing checksums are
   `none` and fail.
7. Expose the execution-time kind and the result of `check_policy` under the
   host policy; proceed only if accepted. Download and compare the
   digest before decompression or replacement; a mismatch aborts the operation.

The manifest does not ask the backend to trust version text as a checksum. The
checksum always belongs to the concrete rendered artifact.

## Registry and mirror model

An artifact registry produces the canonical download URL:

```json
{
  "github-release": {
    "name": "GitHub Releases",
    "url_template": "https://github.com/{owner}/{repo}/releases/download/{release}/{artifact}"
  }
}
```

A metadata registry may omit `{artifact}`:

```json
{
  "github-release-metadata": {
    "name": "GitHub Release Assets Metadata",
    "url_template": "https://github.com/{owner}/{repo}/releases/expanded_assets/{release}"
  }
}
```

A mirror transforms either canonical URL:

```json
{
  "nyanpasu": {
    "name": "Nyanpasu GitHub mirror",
    "url_template": "https://nyanpasu-script.majokeiko.com{path_and_query}",
    "registries": ["github-release", "github-release-metadata"],
    "priority": 10
  }
}
```

Supported mirror variables include:

- `{url}` and `{url_encoded}`;
- `{scheme}`, `{host}`, `{hostname}` and `{port}`;
- `{path}`, `{pathname}`, `{query}` and `{path_and_query}`;
- registry/resource variables such as `{owner}`, `{repo}`, `{release}` and
  `{artifact}`.

The selected mirror is applied consistently to:

- the final binary artifact;
- a remote `version.txt` or equivalent resource;
- a remote checksum list or GitHub release metadata page.

Mirrors affect transport only. Plans preserve `canonical_url` and optionally
provide `mirror_url`; checksum kind is unchanged by which URL the host uses.
Remote version and checksum responses remain parser input, and neither a
canonical URL nor a mirror supplies an implicit checksum or signature coverage.
`priority` is a strategy hint only; the pure resolver does not perform network
probing or speed tests.

## Runtime custom artifact sources

The reference `resolveResourceDownload` API accepts either a named artifact
registry or an inline runtime registry/mirror. User-defined delivery
configuration therefore stays outside the generated manifest:

```ts
resolveResourceDownload(manifest, "mihomo", {
  platform: "windows-x86_64",
  variantId: "stable-goamd64-v2",
  registry: {
    name: "Private object storage",
    url_template: "https://registry.example/{bucket}/{artifact}",
  },
  source: { bucket: "stable" },
  mirror: {
    name: "Custom accelerator",
    url_template: "https://accelerator.example/fetch?url={url_encoded}",
  },
});
```

The shared core's download plan preserves `canonical_url` and optional
`mirror_url`, allowing callers to log the source and retry without acceleration.

## Template value classes

The wire format is unchanged; rendering distinguishes two value classes:

| Class      | Sources                                                                               | Expansion and constraints                                                                                                               |
| ---------- | ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `Authored` | Manifest `source`, registry `variables`, remote `source`                              | Recursively expand values such as `release: "{version}"`; reject cycles, unknown variables, excessive nesting and excessive output size |
| `Literal`  | Parsed remote versions, host-supplied `source` and `version`, URL component variables | Never recursively expand; versions and host variables must satisfy `VersionLabel`; URL components are used only in mirror templates     |

`VersionLabel` matches `^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$`. A value such as
`alpha?x=1#f` is invalid and cannot alter the URL query or fragment. Literal
values are never reinterpreted as templates.

Rendering does not add percent encoding. Valid artifact URLs remain byte-for-byte
equal to the reference implementation. Parse the final URL with the Rust `url`
crate's WHATWG semantics: only http(s) is allowed, and userinfo and fragments are
forbidden.

## Limits

These limits are core constants. Hosts may lower them, but may not raise them.

| Item                                                | Maximum |
| --------------------------------------------------- | ------- |
| Parser body                                         | 2 MiB   |
| A single header value                               | 8 KiB   |
| Regex pattern length                                | 1 KiB   |
| Regex compilation size (`RegexBuilder::size_limit`) | 1 MiB   |
| Template nesting depth                              | 8       |
| Output from a single render                         | 8 KiB   |

Body and single-header-value sizes are measured in UTF-8 bytes. Exceeding
either applicable limit fails with `parser.body_too_large`: `limit` is the
effective byte limit and `actual` is the observed byte count.

The A2 implementation checks these values against real page sizes; any revision
is recorded in the protocol decisions in the associated design spec.

## Error codes

Errors are typed snafu enums with structured fields and a stable `code()` for
each variant. Across languages, consumers identify errors by `code` and fields,
not by parsing `message`. Every error carries a `path` using a single dotted
locator rooted at the input document, at `arg` for host-supplied arguments,
at `response` for parser response errors, or at `parser` for parser rule errors:
join fields or object keys with `.`, and
write array indices as `[<i>]`, for example `manifest_version` or
`resources.<id>.variants[<i>].tags`. Host-supplied arguments use `arg.<name>`,
for example `arg.version`, rather than a bare argument name. Parser response errors
use `response.body` or `response.headers.<lowercase-name>`. Rule errors use
`parser.pattern` for `parser.invalid_pattern`, `parser.group` for
`parser.group_out_of_range`, `parser.pointer` for `parser.invalid_pointer`, and
`parser.type` for `resolve.unsupported_parser`. The conformance
runner compares a declared `path` exactly.

For `parser.ambiguous`, `name` is the requested artifact filename or the declared
header name. For `parser.group_out_of_range`, `groups` includes group 0.

| Prefix      | Codes                                                                                                                                          | Fields                                                                                                                                  |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `manifest.` | `unsupported_major`                                                                                                                            | `{ major }`                                                                                                                             |
| `manifest.` | `invalid_version_format`                                                                                                                       | `{}`                                                                                                                                    |
| `parser.`   | `body_too_large`                                                                                                                               | `{ limit, actual }`                                                                                                                     |
| `parser.`   | `ambiguous`                                                                                                                                    | `{ name }`                                                                                                                              |
| `parser.`   | `group_out_of_range`                                                                                                                           | `{ group, groups }`                                                                                                                     |
| `parser.`   | `not_found`, `digest_missing`, `invalid_version`, `invalid_checksum`, `invalid_encoding`, `invalid_pattern`, `invalid_json`, `invalid_pointer` | Structured fields deferred to the implementing card                                                                                     |
| `template.` | `unknown_variable`, `cycle`, `depth_exceeded`, `output_too_large`, `invalid_literal`                                                           | See [B2 specification §5.6 "B2 渲染错误"](../spec/2026-11-06-manifest-core-parsers/design.md#b2-渲染错误)                               |
| `url.`      | `invalid`, `unsupported_scheme`, `forbidden_component`                                                                                         | See [B2 specification §5.6 "B2 渲染错误"](../spec/2026-11-06-manifest-core-parsers/design.md#b2-渲染错误)                               |
| `resolve.`  | `unknown_resource`, `unsupported_platform`, `variant_not_found`, `tags_rejected`, `selection_conflict`, `default_variant_missing`              | See [B3 specification §5.6 "B3 选择、计划与 VerifyError"](../spec/2026-11-06-manifest-core-parsers/design.md#b3-选择计划与-verifyerror) |
| `resolve.`  | `unknown_mirror`, `mirror_not_allowed`, `snapshot_unavailable`                                                                                 | See [B3 specification §5.6 "B3 选择、计划与 VerifyError"](../spec/2026-11-06-manifest-core-parsers/design.md#b3-选择计划与-verifyerror) |
| `resolve.`  | `version_responses`                                                                                                                            | `{ expected, actual }`                                                                                                                  |
| `resolve.`  | `version_required`, `version_not_in_history`, `unsupported_parser`                                                                             | See [B3 specification §5.6 "B3 选择、计划与 VerifyError"](../spec/2026-11-06-manifest-core-parsers/design.md#b3-选择计划与-verifyerror) |
| `verify.`   | `kind_rejected`                                                                                                                                | `{ kind }`                                                                                                                              |
| `verify.`   | `digest_mismatch`                                                                                                                              | `{ algorithm, expected, actual }`                                                                                                       |
| `validate.` | One code per invariant                                                                                                                         | See [B1 specification §5.6 "B1 校验错误"](../spec/2026-11-06-manifest-core-parsers/design.md#b1-校验错误)                               |

## Validation invariants

`validate_manifest` checks compiled manifests and collects all violations;
compile also rejects invalid templates and generated release data. Each item
identifies its checking boundary:

- a tag or variant ID is invalid or duplicated (`+` is reserved as the tag-key
  separator) — `validate_manifest`;
- a `tag_key` differs from the canonical concatenation of sorted tag IDs —
  `validate_manifest`;
- two variants have the same exact tag combination — `validate_manifest`;
- a variant references an unknown tag or its expanded tag differs from the
  resource-level catalog — `validate_manifest`;
- a variant has two tags from the same non-empty group — `validate_manifest`;
- multiple variants support a platform but no platform default is configured —
  `validate_manifest`;
- a default variant does not exist or does not provide an artifact for that
  platform — `validate_manifest`;
- a static `latest` version is absent from the artifact history —
  `validate_manifest`;
- a history contains duplicate versions — `validate_manifest`;
- the newest captured release, or the captured tag release at the observed
  version, does not contain the rendered artifact — compile
  (`compile.artifact_missing`);
- a checksum does not use a supported `<algorithm>:<hex>` representation —
  snapshot and compiled decoding (`ResourceChecksum`);
- a registry or mirror reference does not exist — `validate_manifest`;
- a known remote parser cannot compile — `validate_manifest` (an unknown parser
  is deferred to artifact resolution as `resolve.unsupported_parser`);
- template variable dependencies are incomplete — `validate_manifest`;
- a template cannot be rendered during compiler preflight — compile.

Compiler preflight is a staged boundary: `validate_manifest` checks only whether
variable dependencies are complete; the compiler then plans every artifact
version and download, canonically and through each mirror, so a template that
cannot render fails at compile time.

If only one variant supports a platform, its default is inferred.

The compiler sorts keys and IDs by Unicode code point rather than
`localeCompare`, so ordering does not depend on runtime locale.

## Compilation

The generator compiles an authored template with a release snapshot:
`compile(template, snapshot, now)` is a pure function, and the same inputs
produce byte-identical output.

- **Template.** Each resource declares named release sources: `history` (the
  newest `limit` releases, 1 to 50) or `tag` (one release, usually a mutable
  prerelease). Each artifact binds to one: a `static` artifact to a history
  source, a `remote` artifact to a tag source. Templates carry no history,
  fallback or checksum, and reject unknown fields.
- **Snapshot.** The generator records what it captured from the repository
  named by the resource's `source.owner` and `source.repo`: each release's ID,
  tag, API URL, publication time and assets (ID, name, digest, update time),
  plus the version it observed for each tag source. IDs are decimal strings.
- **Binding.** A static artifact's history lists every captured release whose
  assets contain the artifact name rendered for that release; the newest one
  must. A remote artifact's fallback is the observed version with the matching
  asset's digest and update time. Without an observed version the result is
  provisional and has no fallback; it only serves to plan the version requests
  that observe the version.
- **Checks.** The result passes `validate_manifest`, then every version and
  download plan is built canonically and through each mirror; a remote
  artifact without a fallback is planned with the placeholder version `0`.

Errors carry a path rooted at `template.`, `snapshot.` or `output.` (the
compiled manifest), with the `compile.*` codes in the
[D2 specification](../spec/2026-11-06-manifest-core-parsers/design.md#96-d2-编译器).

## Consumers

| Consumer                                   | Integration                                                                                            |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| Rust host (clash-nyanpasu backend)         | Direct dependency on `manifest-core`; host implements HTTP and sends `ChecksumKind` to the UI over IPC |
| Node build scripts (downloader, generator) | `@manifest/runtime` through wasm                                                                       |
| Tauri webview                              | Displays backend results and kind; does not load wasm                                                  |
| Pure browser                               | Unsupported                                                                                            |

`manifest-core` and `manifest-parser` perform no IO and depend on no HTTP client
or async runtime. The host performs requests, handles credentials, redirects,
timeouts and response size limits, and supplies body and headers to parsers.
Digest computation and comparison are host responsibilities; a mismatch uses
`verify.digest_mismatch`.

specta is the single source for exported TypeScript domain types. The wasm
boundary uses `serde-wasm-bindgen` with JSON-compatible serialization, reusing
the generated types without tsify. Wasm is intended only for the Node build
scripts in this protocol.

## Migration from manifest v1

Two files are published side by side:

- `manifest/version.json`: unchanged v1 schema and legacy channel keys for
  existing updater/build code, still produced by clash-nyanpasu's upstream
  scripts until they are retired;
- `manifest/resources.json`: v2 schema with one key per core family, channel
  tags, static history and remote resolution instructions, produced by
  `@manifest/generator`, which does not write v1.

This makes schema rollout independent from the updater UI/backend migration.
Once every consumer resolves resources through v2, the legacy `latest`,
`arch_template` and hard-coded GitHub path mapping can be removed.

## Current integration boundary

The template compiler, release-history generator, mirror-aware remote resolver
and parser functions currently exist as reference implementations. The shared
core, parser and wasm integration described here are implemented in stages.
Existing Rust updater and build consumers continue to read
`manifest/version.json` until they are migrated. A Rust consumer migration
depends on `manifest-core`, exposes compatible variants and their checksum kind
through IPC, persists the selected exact tag combination, and verifies the
resolved checksum before decompression.
