# Conformance corpus

Language-neutral cases shared by native Rust and Node (wasm). The corpus began
as the S0b skeleton; that initial stage checked format, global names, and reference
happy paths rather than claiming parser/core implementation. Later runners are
recorded below. Expected behavior follows the normative
[design](../docs/spec/2026-11-06-manifest-core-parsers/design.md) §5, including its
intentional corrections to the reference implementation.

## Case format

Each corpus JSON file is an array of entries such as:

```json
{
  "name": "version.text.trim_false_newline",
  "input": {
    "parser": { "type": "text", "trim": false },
    "response": { "body": "v1.2.3\n", "headers": [] }
  },
  "expect": {
    "error": {
      "code": "parser.invalid_version",
      "path": "response.body",
      "fields": {}
    }
  }
}
```

- `name` is globally unique across directories and corpus JSON files, using dotted
  prefixes. Runners identify cases by the complete name.
  `corpus_case_names_are_globally_unique` recursively checks all corpus directories,
  including any not yet executed. HTML expected lists in `fixtures/` are not corpus
  files and are excluded from that check.
- `input` is always an object, with the shapes below.
- `expect` has exactly one result branch: `{ "ok": <result> }` or
  `{ "error": { "code": "...", "path": "...", "fields": {} } }`. A validation
  error may also declare `expect.additional_errors` containing all remaining errors.
  Every expected error declares its path and complete fields, including `{}`.
  Consumers match stable codes and structured fields, never message text. Runners
  compare full field-object equality and exact path strings.
- Paths are dotted locators from the input document root, joining fields/object
  keys with `.` and indexing arrays as `[<i>]`, for example `manifest_version` or
  `resources.<id>.variants[<i>].tags`. Host parameters use `arg.<name>` such as
  `arg.version`; parser responses use `response.body` or
  `response.headers.<lowercase-name>`. Rule errors locate the input parser:
  `parser.invalid_pattern` -> `parser.pattern`, `parser.group_out_of_range` ->
  `parser.group`, and `resolve.unsupported_parser` -> `parser.type`. Do not use
  bare parameter names or another path syntax.
- Compare JSON structure: object-key order is irrelevant, array order is retained.
  Do not pre-trim/normalize inputs or discard duplicate headers. Validation adds
  no synthetic aggregate code in corpus results: combine the first and additional
  errors into a multiset of code/path/full-fields. Error ordering is irrelevant,
  but extra, missing, and duplicate occurrences still matter.
- Successful checksums serialize `ResourceChecksum` as
  `<algorithm>:<lowercase-hex>`. SHA-256 uses 64 hex characters (32 bytes); base64
  inputs represent the same 32 bytes.

| Directory/file           | Input and operation                                                                                                                                           | `expect.ok`                             |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| `parsers/version.json`   | `{ parser, response: { body, headers } }`; compile and execute the version parser.                                                                            | Version string                          |
| `parsers/checksum.json`  | `{ parser, response: { body, headers }, artifact }`; parse for the artifact filename.                                                                         | Checksum string                         |
| `validate/manifest.json` | Complete compiled manifest; consumer decoding then `validate_manifest`, without adding fields or loading/merging external fixtures.                           | `true`                                  |
| `compat/manifest.json`   | Complete compiled manifest; permissive decoding against runner `CURRENT_MINOR` (currently 0), then per-artifact support checks below, without filling fields. | Compatibility projection                |
| `kind/artifact.json`     | Artifact-template wire object `{ artifact, source?, version }`; call `artifact_checksum_kind` without a request/response.                                     | Kind string                             |
| `resolve/templates.json` | B2 explicit artifact or generic template rendering, described below.                                                                                          | `{ artifact, canonical_url }` or string |

`response.headers` is an array of `[name, value]` pairs, for example
`[["X-Checksum-Sha256", "..."], ["x-checksum-sha256", "..."]]`. Preserve casing,
order, and repeated values rather than converting to an object that merges them.
Parser objects use manifest `type` tags and field names: version `text`/`regex`;
checksum `checksum`/`gnu`/`bsd`/`github-release-assets` and the added `header`.
Header encoding is `hex` or `base64`, with a lowercase algorithm name. Version
cases include headers even though version parsers only read the body. Checksum
`artifact` is a concrete filename, not a `{version}` template.

`checksum.header.invalid_base64` expects `parser.invalid_encoding`.
`checksum.header.invalid_hex` and `.wrong_length` expect `parser.invalid_checksum`;
non-hex characters, odd hex length, and wrong digest length all use that code.
A received header value exceeding its effective limit (default 8 KiB, lowerable by
hosts) uses `parser.body_too_large`, like body overflow, with `{ limit, actual }`
in UTF-8 bytes. A2/A5 added limit consistency coverage.

### Resolve templates (B2)

`crates/manifest-core/tests/resolve.rs` runs `resolve/templates.json`; all names
remain covered by parser's recursive uniqueness check. B4 goldens are separate.

Artifact input is `{ manifest, resource, variant, platform, version?, source?, limits? }`.
Manifest is complete or exactly `{ "snapshot": "resources.json" }`, which loads
`crates/manifest-metadata/tests/fixtures/resources.json`; no other path or field
merge/override is accepted. Decode and validate first, then fetch the explicit
resource/variant/platform. Version/source are host Literals; version may come from
a remote parser. Static defaults to latest; remote requires a version. Matching
static releases or remote fallbacks contribute their source. Success is exactly
`{ artifact, canonical_url }`; errors compare code, path, and complete fields.

Generic template input is `{ template, values, path, limits? }`, with every value
exactly `{ "authored": "..." }` or `{ "literal": "..." }`; success is a string.
These cases call `render_template` directly, including unknown dependencies B1
would reject earlier and Literals with URL punctuation/braces for future mirrors.
Generic Literals do not enforce host character restrictions. The supplied path
locates template errors; this file uses `arg.template`, not an invented manifest
locator.

Both forms accept only `template_depth` and `render_output_bytes` in limits, using
lower-only `Limits::with_*` methods. Missing values use defaults. Root depth is 0;
entering Authored adds 1, Literal adds no depth. Output is checked before each
append in UTF-8 bytes; actual is the current prefix plus the first overflowing
segment, not the fully expanded result. Cases cover positive/negative default
and lowered boundaries and raised requests clamped to defaults.

`resolve.host_version_injection` rejects `alpha?x=1#f` at `arg.version`.
`resolve.authored_release` expands `release: "{version}"` recursively. Other cases
cover every precedence layer, non-rescanning of Literals, DFS, all template/URL
errors, URL normalization, preserved percent escapes and `+`, authored queries,
and rejection of empty userinfo/fragments. The snapshot regression renders all
48 resource × variant × platform × static-history URLs, asserting no canonical
URL contains `?` or `#`, without network access.

### Parser limit coverage (A2)

Parser cases may add `input.limits`. Missing values use defaults. Values are
nonnegative integers named `body_bytes`, `header_value_bytes`,
`regex_pattern_bytes`, `regex_size_bytes`, `template_depth`, and
`render_output_bytes`. Runners lower through `Limits::with_*`; values above the
current limit clamp to it and cannot raise defaults. Unknown fields are rejected.
A3/A5 checksum cases reuse this extension.

For example, `"limits": { "body_bytes": 3 }` with `"body": " v1\n"` yields
`parser.body_too_large`, fields `{ "limit": 3, "actual": 4 }`, path `response.body`,
without embedding a 2 MiB body in the corpus. `version.text.body_limit_utf8` covers
multibyte counting; Rust regression tests cover the default 2 MiB boundary.

`checksum.header.raised_limits_clamped` requests 33,554,432 for body/header limits
with an 8,193-byte literal header, but still expects `parser.body_too_large` and
`{ "limit": 8192, "actual": 8193 }`. This small header proves raised-limit clamping.
Rust `limits_can_only_be_lowered` verifies the body default cannot be raised,
without a new corpus generation syntax or a large literal body.

`version.regex.pattern_limit` and `.pattern_limit_utf8` check UTF-8 pattern bytes,
expecting `parser.invalid_pattern` with `{ limit, actual }`.
`version.regex.compiled_size_limit` triggers the default 1 MiB compiled-size limit
with a short pattern; `.compiled_size_lowered` checks host lowering. Regex syntax
or compiled-size failures have `{}` fields; dependency message text is not compared.
Version-not-found and invalid-version fields are also `{}`.

Version parsers do not support headers; the helper is not a version parser type.
`version.text.ignores_conflicting_headers` verifies they are ignored. Rust integration
tests cover helper case-insensitive names, identical duplicates, conflicts, absence,
and lowercase header paths. A5 adds checksum-header corpus paths.

### Checksum stages and error fields (A3/A4/A5)

Rust executes every case in `parsers/checksum.json`: `checksum`, `gnu`, `bsd`,
`github-release-assets`, `header`, and unknown types, without skips. Print full
names with `cargo test -p manifest-parser --test conformance -- --nocapture`.

Non-header `RemoteChecksumParser::parse` checks UTF-8 body bytes before dispatch,
so overflow takes precedence as `parser.body_too_large`. `checksum`, `gnu`, and
`bsd` only read body, ignoring header values/conflicts/sizes and never falling back
to headers for a missing or invalid body.

Header parsing reads only the declared name, ignoring body size/content and other
headers. It matches names without ASCII case sensitivity and checks every received
matching value's UTF-8 size before decoding or duplicate comparison. Any overflow
wins, even with conflicting duplicates or invalid encoding. Identical candidate
strings deduplicate; differently cased hex candidates conflict before normalization.
The A5 notes described raw value comparison and host-side transport-whitespace
removal. The current §5.3/V-15 contract also accepts client-merged comma lists:
split each received value, trim SP/HTAB around elements, and ignore empty elements.
No elements produces not-found. Check the 8 KiB received-value limit before splitting.

Base64 uses the RFC 4648 standard `+`/`/` alphabet, canonical `=` padding when
needed, and zero padding bits. Reject URL-safe symbols, missing padding, and
whitespace within the encoded candidate. The original strict encoding notes also
reject leading/trailing whitespace before transport/list OWS removal; hex decoding
itself does not trim. HTTP hosts remove transport whitespace when constructing
`RemoteResponse`; current core list handling removes element OWS as specified.
Header `not_found`/`ambiguous` fields are `{ name }` preserving the rule's name;
`invalid_encoding`/`invalid_checksum` fields are `{}`. All header response paths
use `response.headers.<ASCII-lowercase-name>`. Corpus cases cover lowered limits,
UTF-8, encoding failures, and paths; Rust regressions also cover the default 8 KiB
boundary and a valid header with a body above 2 MiB.

Metadata validates header names when decoding `ResourceVariantTemplate` artifacts:
nonempty RFC 9110 tokens, ASCII letters/digits and ``!#$%&'*+-.^_`|~``. Compiled
manifest decoding stays permissive; A5 did not reject the entire manifest for an
invalid name, leaving B1 semantic validation to check it. Shared parser schema
adds no header-name pattern, preserving the template-only boundary. A5 generated
changes added the header variant and `HeaderEncoding`.

GNU/BSD split on `\n`, removing one trailing `\r` per line. GNU uses
`<hex>[ \t]+[* ]?<name>[ \t]*` and full artifact-name matching, preserving internal
spaces/special characters and the artifact's own leading/trailing spaces. It tries
at most two name starts: without a marker, account for artifact leading whitespace;
with `*`, start after it. A space marker is equivalent to separator whitespace.
All-whitespace names are located linearly within the separator. Each candidate
must start with the complete artifact and have only space/tab afterward. Names
are not pre-trimmed or searched at every occurrence; line parsing is linear.

BSD uses fixed `<LABEL> (<name>) = <hex>` delimiters and exact names. Labels compare
without ASCII case sensitivity; wrong-algorithm lines are skipped. Both list parsers
skip malformed, non-hex, or wrong-length lines even for matching names, consistent
with the reference's fixed-length hex regex. Only valid candidates participate in
deduplication: equal normalized digests merge, different ones are ambiguous, and
no valid candidates is not-found. Neither accepts an empty artifact, even for
`<hex>  ` or `SHA256 () = <hex>`: `artifact: ""` gives not-found with `{ name: "" }`.
Nonempty all-space/tab names still match the full name.

List not-found/ambiguous fields are `{ name }` with the requested artifact unchanged;
raw `checksum` invalid-checksum fields are `{}`. All these paths are `response.body`.

GitHub parsing uses lightweight `tl` HTML `li` entries. Decode href HTML references
once, remove query/fragment, take the last path segment, and percent-decode once
for exact matching. Ignore display text and preserve literal `+`. A deliberately
limited character-reference table controls wasm size rather than shipping the
complete WHATWG table. Supported references follow
[WHATWG attribute-value rules](https://html.spec.whatwg.org/multipage/parsing.html#named-character-reference-state):

- Decimal `&#N;` and hex `&#xH;`/`&#XH;` consume consecutive valid digits, with an
  optional semicolon. Apply the 0x80–0x9F C1 replacement table from the
  [numeric-reference rules](https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state),
  e.g. `&#128;` -> `€`; unmapped values retain their code point. Zero, surrogate
  points, values above U+10FFFF, and integer overflow become U+FFFD.
- Semicolon-terminated named references include `amp`, `lt`, `gt`, `quot`, `apos`,
  `nbsp`; ASCII punctuation `num`, `sol`, `colon`, `quest`, `equals`, `period`,
  `plus`, `percnt`, `excl`, `commat`, `lpar`, `rpar`, `lsqb`, `rsqb`, `lowbar`,
  `Hat`, `grave`, `verbar`, `tilde`; and uppercase `AMP`, `LT`, `GT`, `QUOT`.
- Only `amp`, `lt`, `gt`, `quot` and those uppercase forms allow a missing semicolon,
  and only before a non-ASCII-alphanumeric character other than `=`, or at the
  attribute end. Thus `a&amp.zip` becomes `a&.zip`, but `a&ampx` and `a&amp=b` stay
  unchanged.

Other references such as `&copy;` stay literal. Scanning is linear, single-pass,
and non-recursive. After reference decoding, remove query/fragment from the full
href, then require `/releases/download/` in its path. That substring only in a
query/fragment is insufficient; `&sol;` can change real path boundaries.

Root entry traversal prunes scripts, styles, and comments. Digest text is collected
from `.Truncate-text` in the same entry, skipping nested li/script/style/comments.
A container with only excluded content has no digest; retained valid text beside
comments/scripts still produces its digest. No name match gives not-found; a
matching entry without a digest gives digest-missing; conflicting digests within
or across same-name entries give ambiguous. All three carry `{ name }`.
Deduplicate after algorithm/hex normalization. Any matching digest-less entry
rejects, even when another matching entry has a digest. Unsupported algorithms,
invalid hex/length, or empty digests give invalid-checksum with `{}`. Unmatched
entries' digests are not validated. All paths are `response.body`; body limits
are checked before HTML parsing. A `tl::parse` failure maps to invalid-checksum,
`{}`, `response.body`, without deriving a new code from tokenizer text.

Temporary `ParseError::Unsupported` was removed. Unknown types use
`ResolveParser { parser_type }`, yielding `resolve.unsupported_parser`,
`{ parser_type }` with the original tag, and `parser.type`.

Version cases (`kind` plans with `operation: "version"`) pass their responses to
`finish_version`. `responses` lists one response per plan part; the older
`response` is shorthand for a single-element list. A response may give
`body_file`, a path under `conformance/`, instead of an inline `body`; the
[GitHub API fixtures](fixtures/github-api/README.md) are used this way.

The [GitHub fixture README](fixtures/github-expanded-assets/README.md) records
real URLs/UTC times, independent lists, and O-3 size measurements.
`github_expanded_assets_fixtures` walks every HTML file and adjacent fixed
`*.expected.json`, checks each digest asset, and requires errors for old-page
assets without digests: four fixtures, 219 assets, 191 digests. The encoded fixture
is explicitly synthetic; real HTML remains untouched. Recheck with
`cargo test -p manifest-parser --test conformance`.

A3 directly reran the read-only reference functions: 24 happy-path results matched
the corpus. Intentional differences remain S0b's GNU same-name conflicts and BSD
wrong labels. Added `checksum.bsd.skips_other_algorithm` verifies skipping a wrong
label and returning a later correct digest; the reference incorrectly takes the
first line. The pre-existing 18 checksum cases retained names, inputs, and expectations.

### Validation stages

Positive cases require success. Rust additionally checks round-trip preservation
and `newer_minor`; those are implementation checks rather than corpus contracts.
`validate.manifest.reference_fixture` embeds a complete structural copy of the
[`resources.json`](../crates/manifest-metadata/tests/fixtures/resources.json)
snapshot, both with string version `"2.0"`. Runners consume the case input directly
without implicit migration; a separate regression also loads the snapshot itself.

B1 `crates/manifest-core/tests/conformance.rs` recursively executes every validate
and compat JSON file. `validate/invariants.json` adds 61 cases to the existing
snapshot case: 62 total, 21 positive and 41 negative, covering design §7.1's ten
invariant groups and G6. Names below omit `validate.`; full codes/fields are in
[design §5.6](../docs/spec/2026-11-06-manifest-core-parsers/design.md).

| Invariant/code suffix | Positive cases                                                              | Negative examples                                                                                                                                                                                    |
| --------------------- | --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ids`                 | `ids.valid`                                                                 | `ids.invalid_variant`, `ids.duplicate_catalog`, `ids.duplicate_variant_tag`, `ids.duplicate_variant`                                                                                                 |
| `tag_key`             | `tag_key.canonical`                                                         | `tag_key.forged`, `tag_key.unsorted`                                                                                                                                                                 |
| `expanded_tag`        | `expanded_tag.consistent`                                                   | `expanded_tag.unknown`, `expanded_tag.mismatched_name` / `group` / `description`                                                                                                                     |
| `tag_group`           | `tag_group.empty_and_distinct`                                              | `tag_group.conflict`                                                                                                                                                                                 |
| `tag_combination`     | `tag_combination.distinct`                                                  | `tag_combination.duplicate`                                                                                                                                                                          |
| `default_variant`     | `default_variant.multiple_configured`, `default_variant.single_inferred`    | `default_variant.missing` / `unknown` / `unsupported_platform`                                                                                                                                       |
| `reference`           | `reference.existing`                                                        | `reference.resource_registry_missing`, `reference.remote_registries_missing`, `reference.mirror_registry_missing`                                                                                    |
| `static_history`      | `static_history.latest_not_first`                                           | `static_history.latest_missing`, `static_history.duplicate`                                                                                                                                          |
| `parser_compile`      | `parser_compile.known`, `parser_compile.unsupported_accepted`               | `parser_compile.pattern` / `group` / `pattern_limit` / `header_name`                                                                                                                                 |
| `template_variable`   | `template_variable.complete`, `template_variable.release_and_remote_scopes` | `template_variable.artifact_missing` / `registry_missing` / `nested_missing` / `release_missing` / `remote_source_missing` / `remote_artifact_missing` / `remote_artifact_absent` / `mirror_missing` |

`all_violations.collected` triggers seven violations across invariants, checking
collection rather than early return. Duplicate-tag cases also assert duplicate
IDs and noncanonical keys. `template_variable.cycle_deferred` confirms B1 only
checks dependency completeness; B2 handles cycles. `artifact_alias_complete` and
`remote_artifact_alias_complete` allow registry variables to reference the rendered
artifact. `artifact_source_dependency_missing` still checks every dependency of
source values actually used by artifact templates, locating the source field
containing the missing reference.

Uninstantiated registries may retain context variables. B1 does not render, validate
URLs, or require values before their execution stage; builtin `version` counts as
a runtime source, preserving existing compat cases. Checksum has no compile API:
B1 prechecks header names, with other configuration constraints in types. Version
compile uses default Limits, wrapped as `validate.parser_compile` preserving its
original code/fields. Wire manifests have no named mirror references; mirrors'
registry allow lists must refer to known registries.

Print full names/counts with
`cargo test -p manifest-core --test conformance -- --nocapture`. Original
`validate/manifest.json` and `compat/manifest.json` names/inputs/expectations were
unchanged; parser's uniqueness check recursively covers the additions.

### Compatibility projection

Compat cases each contain resource `sample` and variant `stable`. After decoding,
validate the manifest, then preflight parser support for each retained known-platform
artifact: compile remote version parsers and check checksum-parser support. No
HTTP, remote version value, or download-time acceptance policy is required.
Static artifacts and remote artifacts whose parsers are all supported project
`{ "ok": true }` with a sibling `kind` from design §5.1 rule 3. Unknown version or
checksum parsers affect only that artifact:
`{ "error": { "code": "resolve.unsupported_parser", "fields": { "parser_type": "..." } }, "kind": "none" }`.
Other artifacts continue; no policy accepts none. Unknown parsers must not fail
whole-manifest decoding or semantic validation.

Full projection:

```json
{
  "newer_minor": true,
  "default_variants": {
    "windows-x86_64": "stable",
    "linux-amd64": "stable"
  },
  "artifacts": {
    "windows-x86_64": {
      "error": {
        "code": "resolve.unsupported_parser",
        "fields": { "parser_type": "future-version-parser" }
      },
      "kind": "none"
    },
    "linux-amd64": { "ok": true, "kind": "authoritative" }
  }
}
```

`default_variants` is the complete decoded `resources.sample.default_variants` map;
`artifacts` lists exactly the decoded `resources.sample.variants[0].artifacts` keys.
Unknown `future-os-cpu` must be skipped from both. Runners also check preservation
of known resource/variant tags, sources, and supported artifacts' wire data.
`newer_minor` uses runner `CURRENT_MINOR` (currently 0): `"2.7"` is true, `"2.0"`
false. No leading-zero minor: `compat.version.leading_zero` expects
`"2.07"` -> `manifest.invalid_version_format`. Version-decoding failures use top-level
`expect.error` without per-artifact checks. The combined
`compat.newer_minor.unknown_platform_and_parser` directly covers G8.

### Kind names and static inference

Kinds are `authoritative`, `parsed`, `snapshot`, and `none`, the fixed lowercase
snake_case wire names from design §5.1; Rust uses
`#[serde(rename_all = "snake_case")]`. This is a wire contract, not merely corpus
style. Existing `github-release-assets` parser tags remain kebab-case.

The initial static-kind notes describe eight cases: embedded static-release
checksum -> authoritative, static without checksum -> none, remote checksum rule
-> parsed, remote without a rule -> none. Static tests cover latest not being
first and historical checksum differing from latest; static display uses latest.
Both remote cases include checksummed fallbacks but still infer parsed/none from
rule presence. Snapshot is the execution result of explicit offline fallback
with parsed version exactly equal to `fallback.version`, not a static rule-3
result, so static artifact cases do not expect snapshot. Unknown version/checksum
parsers infer none.

### Kinds, selection, and plans (B3)

Core's conformance runner recursively executes all kind JSON: 88 cases at B3,
with 83 added there. Compatibility projections call core's static kind and version
planning directly and assert per-artifact support; unknown-parser artifacts also
check that every policy combination rejects none.

| File                  | Input/operation                                                                                                    | `expect.ok`                                     |
| --------------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------- |
| `kind/artifact.json`  | Artifact wire object, no operation; static inference.                                                              | Kind string                                     |
| `kind/policy.json`    | `{ operation: "policy", kind, policy }`, or `digest_mismatch` with algorithm/expected/actual.                      | True or standard error                          |
| `kind/selection.json` | `{ operation: "select" or "list", manifest, resource, platform, variant_id?, tags? }`.                             | `{ variant_id, kind }` or array                 |
| `kind/plans.json`     | Common fields above, operation `version`/`download`, optional version/source/mirror/checksum_mode/limits/response. | Complete serialized plan or `{ plan, version }` |
| `kind/finish.json`    | `finish_checksum`: plan a download then complete checksum with response.                                           | `{ checksum, kind: "parsed" }`                  |

Manifests are completely embedded without merging; decode/validate before selecting.
Names remain globally unique. Tags select exact combinations independent of order;
empty or duplicate tags are rejected, and multiple selectors must agree. Lists
retain variant order and include only platform-available artifacts; no candidates
means an empty list but a selection error.

Plan fields follow [design §7.3](../docs/spec/2026-11-06-manifest-core-parsers/design.md).
Optional requests/checksums serialize as null; native compiled parsers and Limits
are not exported. A version operation with response actually executes its compiled
parser and compares `{ plan, version }`. `finish_checksum` preserves explicit
parser provenance without inferring from other headers. List parsers match the
real artifact filename, not a plan's virtual checksums.txt. Response headers remain
pair arrays. Limits lower body_bytes/header_value_bytes/render_output_bytes/
regex_pattern_bytes, defaulting otherwise.

Missing/remote checksum mode is online. Snapshot is explicit offline use, requiring
an exact fallback version and embedded checksum. Fallback checksums do not affect
static kind and are never used automatically by online plans. Static download kind
uses the selected release. Unknown parsers also reject snapshot mode. Mirror cases
cover absent/empty/matching/nonmatching registry lists, including separate version
and checksum registries, confirming B1 checks registry scope only. Component
replacement, url_encoded, URL safety, and builtin precedence are compared exactly.
Policy cases exhaust four kinds × two bool switches: all 16 combinations.

### Reference resolve differential (B4)

`resolve/reference-golden.json` uses the same named-case format with input
`{ manifest: { snapshot: "resources.json" }, resource, platform, variant, version }`
and success `{ artifact, canonical_url, download_url }`. Object keys recursively
sort by code point; arrays sort by resource/platform. Files use LF and Prettier JSON.

Reference source is upstream
[libnyanpasu/clash-nyanpasu](https://github.com/libnyanpasu/clash-nyanpasu)
`scripts/resource-template.ts`, using the read-only vendored copy at
`files/scripts/resource-template.ts`. No Git metadata identifies its upstream
commit. Its SHA-256 is
`0a02398a48d35465e4cf23e54abf8777d2d0f4b8a5543f033d716c8974069213`.
The reference module has no runtime imports. Deno uses `--no-config --no-lock`
to avoid writes from automatic node_modules configuration in
`files/scripts/deno.jsonc`.

The reference still expects integer manifest_version 2. The one-off script adapts
only an in-memory copy under V-9, preserving snapshot `"2.0"`. Resource platforms
are the union of all variant artifact platforms, resolved through reference defaults.
Static omits version and uses latest; remote explicitly passes fallback.version.
Missing default artifacts/fallbacks fail immediately. No mirror is selected, so
both URLs must match. The snapshot covers 3 resources × 6 platforms: 18 defaults,
all static and none remote. All 54 artifact-name/URL byte comparisons matched,
with no new deviation.

`crates/manifest-core/tests/reference_golden.rs` compares each result through public
`select_variant` (default selection) and `plan_download` (no mirror), checks complete
resource/platform coverage without duplicates, and checks chosen variant/version.
The B2 resolve runner explicitly excludes this file for the independent differential
runner. Parser's global name check still covers every golden entry.

Rebuild with these complete commands from the root in Git Bash. The one-off script
lives in a temporary directory, outside version control. Reading the reference
needs no network; normal tests require neither Deno nor `files/`.

```bash
printf '%s\n' '0a02398a48d35465e4cf23e54abf8777d2d0f4b8a5543f033d716c8974069213  files/scripts/resource-template.ts' | sha256sum --check
b4_scratch="$(mktemp -d)"
cat > "$b4_scratch/generate.ts" <<'TS'
const root = new URL(`file:///${Deno.cwd().replaceAll("\\", "/")}/`);
const { RESOURCE_MANIFEST_VERSION, resolveResourceDownload } = await import(
  new URL("files/scripts/resource-template.ts", root).href
);
const snapshot = JSON.parse(
  await Deno.readTextFile(
    "crates/manifest-metadata/tests/fixtures/resources.json",
  ),
);
if (snapshot.manifest_version !== "2.0") {
  throw new Error("expected snapshot manifest_version 2.0");
}
const manifest = { ...snapshot, manifest_version: RESOURCE_MANIFEST_VERSION };
const cases = [];
let remote = 0;
for (const resource of Object.keys(snapshot.resources).sort()) {
  const definition = snapshot.resources[resource];
  const platforms = [...new Set<string>(
    definition.variants.flatMap((variant: { artifacts: object }) =>
      Object.keys(variant.artifacts)
    ),
  )].sort();
  for (const platform of platforms) {
    const variant = definition.default_variants[platform];
    const selected = definition.variants.find(
      (entry: { id: string }) => entry.id === variant,
    );
    const artifact = selected?.artifacts[platform];
    if (!artifact) throw new Error(`${resource}/${platform}: missing default`);
    const isRemote = artifact.version.type === "remote";
    const version = isRemote
      ? artifact.version.fallback?.version
      : artifact.version.latest;
    if (!version) throw new Error(`${resource}/${platform}: missing version`);
    const result = resolveResourceDownload(manifest, resource, {
      platform,
      ...(isRemote ? { version } : {}),
    });
    if (result.variant.id !== variant || result.version !== version) {
      throw new Error(`${resource}/${platform}: unexpected selection`);
    }
    if (result.canonical_url !== result.download_url) {
      throw new Error(`${resource}/${platform}: unexpected mirror`);
    }
    cases.push({
      name: `resolve.reference.${resource}.${platform}`,
      input: {
        manifest: { snapshot: "resources.json" },
        resource,
        platform,
        variant,
        version,
      },
      expect: {
        ok: {
          artifact: result.artifact,
          canonical_url: result.canonical_url,
          download_url: result.download_url,
        },
      },
    });
    if (isRemote) remote += 1;
  }
}
function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
        .map(([key, entry]) => [key, sorted(entry)]),
    );
  }
  return value;
}
await Deno.writeTextFile(
  "conformance/resolve/reference-golden.json",
  `${JSON.stringify(sorted(cases), null, 2)}\n`,
);
console.log(
  `${Object.keys(snapshot.resources).length} resources, ${cases.length} entries (${remote} remote, ${cases.length - remote} static)`,
);
TS
deno run --no-config --no-lock \
  --allow-read=files/scripts/resource-template.ts,crates/manifest-metadata/tests/fixtures/resources.json \
  --allow-write=conformance/resolve "$b4_scratch/generate.ts"
pnpm exec prettier --write conformance/resolve/reference-golden.json
```

## Sources and reference checks

- Normative [design](../docs/spec/2026-11-06-manifest-core-parsers/design.md)
  §1.2/§1.3, §5.1–§5.4, §5.6 and the
  [S0b task card](../docs/spec/2026-11-06-manifest-core-parsers/tasks.md).
- Wire fields/tags/names:
  [`crates/manifest-metadata/src/lib.rs`](../crates/manifest-metadata/src/lib.rs).
- Positive validation snapshot:
  [`resources.json`](../crates/manifest-metadata/tests/fixtures/resources.json).
  Compat manifests are handwritten reductions of its registries, resources,
  expanded tags, variants, and artifacts.
- Read-only upstream `parseRemoteVersion` and `parseRemoteChecksum` in
  `files/scripts/resource-template.ts`, approximately lines 1211–1280. `files/`
  is untracked and not a runner dependency.
- Inline GitHub HTML in `parsers/checksum.json` is deterministic synthetic input.
  A4 real pages live separately under `fixtures/github-expanded-assets/`; its
  README records provenance, independent expectations, and the one synthetic fixture.

At creation, a one-off Node script used native TS stripping to import the read-only
reference and compared these happy paths:

- `version.text.trim_default`, `version.text.trim_false_clean`
- `version.regex.capture`, `version.regex.group_zero`
- `checksum.checksum.normalized`, `checksum.gnu.exact_name`
- `checksum.bsd.matching_label`, `checksum.github_release_assets.own_digest`

It called `parseRemoteVersion(input.response.body, input.parser)` or
`parseRemoteChecksum(input.response.body, input.parser, input.artifact)` and compared
with `expect.ok` via `node:assert/strict`, without modifying upstream files.
Header parsing has no upstream implementation; expectations follow the normative
contract and base64 byte decoding.

`checksum.github_release_assets.percent_encoded_href` is a normative case:
`foo%2Bbar.zip` becomes `foo+bar.zip`, with deliberately unrelated display text,
and returns that entry's digest. The old reference need not pass this correction.
Bug cases use corrected expectations. Additional reference runs confirmed both
GitHub traps return a neighboring/prefix asset's digest and a wrong BSD label
is incorrectly treated as configured SHA-256.

## Defects and cases

This table covers every design §1.2/§1.3 defect marked as reproduced by Codex,
plus S0b's other required corrections. The two GitHub reproductions are separate.

| Source/defect                                                                       | Case                                            | Corrected expectation                            |
| ----------------------------------------------------------------------------------- | ----------------------------------------------- | ------------------------------------------------ |
| §1.2: `alpha?x=1#f` changes URL query/fragment (Codex reproduction).                | `version.text.query_fragment`                   | `parser.invalid_version`, before URL generation. |
| §1.3: selected asset has no digest but reads its neighbor's (Codex reproduction 1). | `checksum.github_release_assets.digest_missing` | `parser.digest_missing`                          |
| §1.3: `selected.zip` matches `prefix-selected.zip` (Codex reproduction 2).          | `checksum.github_release_assets.exact_name`     | Only `selected.zip`'s own digest.                |
| §1.3: text `trim: false` still trims unconditionally.                               | `version.text.trim_false_newline`               | `parser.invalid_version`                         |
| §1.3: BSD does not check algorithm labels.                                          | `checksum.bsd.mismatched_label`                 | Skip SHA512; `parser.not_found`.                 |
| §5.3/S0b: conflicting GNU same-name digests.                                        | `checksum.gnu.conflicting_digests`              | `parser.ambiguous`                               |
| §5.3/S0b: regex group outside capture count.                                        | `version.regex.group_out_of_range`              | `parser.group_out_of_range` during compile.      |
| §5.1/S0b: implicit Digest without a header rule.                                    | `checksum.checksum.ignores_digest_header`       | Body checksum; ignore the conflicting header.    |

The exact-name trap puts a different-digest `prefix-selected.zip` before
`selected.zip`, within the old implementation's 2048-character window; the old
proximity regex returns the former. The missing-digest trap places another
checksummed asset immediately after digest-less `selected.zip`. The BSD trap
labels a 64-hex-character, 32-byte digest (valid SHA-256 length) as SHA512 to expose
the missing-label check; it is malformed input, not a valid SHA-512 example.

## Verification

### Rust/Node executed-name parity (C3)

Run from the repository root:

```sh
pnpm run test
pnpm run check:conformance
```

Both build wasm first, generating runtime glue on fresh checkouts. The test task
runs `packages/*/*_test.ts`. Node's
`packages/manifest-runtime/conformance_test.ts` registers `test(name)` for every
corpus case and every GitHub fixture asset, executing through `@manifest/runtime`.
After its wasm build, the conformance task runs:

```sh
cargo test -p manifest-parser --test conformance
cargo test -p manifest-core --test conformance --test resolve --test reference_golden
node --test packages/manifest-runtime/conformance_test.ts
```

`scripts/src/conformance/check-conformance.ts` inherits the caller environment,
including `RUSTUP_TOOLCHAIN`. It creates separate empty temporary Rust/Node lists
and passes their paths as `MANIFEST_CONFORMANCE_LIST`. Each side appends one JSON
string only after a case passes, retaining its full name. Fixture names are
`fixtures.github-expanded-assets.<fixture stem>.<asset name>`. Without that
variable, runners write no list.

Any child failure, empty list, same-side duplicate, or name-set difference fails
the task. Differences print names missing on Rust and Node separately. Success
prints per-directory/prefix counts and totals; version/checksum prefixes map to
`parsers/version` and `parsers/checksum`. Lists are cleaned on success and failure,
and each run uses a fresh directory so old lists cannot affect it.

A negative check can temporarily skip one complete case name inside Rust parser's
`run_cases` loop, then verify `pnpm run check:conformance` fails with that name in
`Missing on Rust`. Restore the runner and verify it passes again.

### Corpus format verification (S0b)

S0b used a one-off root-level Node script to traverse `conformance/**/*.json`,
`JSON.parse` each file, require an array, check globally unique names, and print
per-file counts. The script was not checked in; its full text is in the execution
report. It then ran:

```sh
pnpm exec prettier --write conformance
pnpm run fmt:check
```

A2/A3 added Rust parser execution; B1/B3 added validate/kind execution; B2 filled
`resolve/templates.json`; B4 added reference differentials; C3 made Node read the
same data and compare executed-name lists.
