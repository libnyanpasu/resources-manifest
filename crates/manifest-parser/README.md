# manifest-parser

Resource parser definitions and execution without IO. Compile version rules with
`RemoteVersionParser::compile`, then process a borrowed `RemoteResponse` with
`CompiledVersionParser::parse`. `VersionLabel` accepts only
`^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$` and does not trim during conversion.

## Limits and responses

`Limits` has private fields and the six defaults in design §5.5. Its chainable
`with_*` methods clamp requested values to the current limit: requests above the
default or attempts to raise an already lowered value retain the current value.
Zero is allowed. Template-depth and render-output limits also live here for core
rendering (B2). Body, header-value, and regex-pattern lengths count UTF-8 bytes.
Compiled version parsers retain the effective body limit and check it before
trimming or matching on every parse.

`RemoteResponse::header_values` returns original values using ASCII
case-insensitive names. `single_header` deduplicates exactly identical values
without modifying them; absence produces `parser.not_found` and different values
produce `parser.ambiguous`. Version parsers read only the body; the checksum
`header` parser handles header size and encoding checks.

## Errors

`ParseError` variants expose matchable fields, and `fields()` returns the stable
JSON field object:

- `body_too_large`: `{ limit, actual }`.
- `group_out_of_range`: `{ group, groups }`, with group 0 included in `groups`.
- `unsupported_parser`: `{ parser_type }`.
- `invalid_pattern`: `{ limit, actual }` for pattern-byte overflow, or `{}` for
  syntax/compiled-size errors, retaining the regex error as the source.
- Version `not_found` and `invalid_version`: `{}`.
- Header-helper `not_found` and `ambiguous`: `{ name }`, preserving the declared
  name. An optional capture absent from the first regex match is `parser.not_found`;
  later matches are not tried.

`path()` returns `response.body` for body errors and
`response.headers.<ASCII-lowercase-name>` for header errors. Rule errors use
`parser.pattern` for overflow/compilation failure, `parser.group` for an invalid
capture group, and `parser.type` for an unknown parser, matching the corpus input's
`parser` object.

## Checksum rules

`RemoteChecksumParser::parse` implements all five rules:

- `checksum`: trim the body and parse `<algorithm>:<hex>`, normalizing algorithm
  and hex casing.
- `gnu`: match the full original artifact name in GNU lists, checking separators
  and trailing spaces/tabs separately.
- `bsd`: match the filename exactly and compare algorithm labels without ASCII
  case sensitivity.
- `github-release-assets`: read download links and digests from the same HTML `li`
  asset entry. Skip scripts, styles, and comments; do not read digests across nested
  entries. Decode HTML references in href once, remove query/fragment, then extract
  the last path segment and percent-decode once for exact matching.
- `header`: read only the declared header, decode hex or standard base64, and check
  the received-value size limit. Ignore body and unrelated headers. As specified by
  design §5.3, split delivered comma lists, trim SP/HTAB around elements, and ignore
  empty elements before comparing candidates.

GNU/BSD and GitHub deduplicate equal normalized digests; conflicts produce
`parser.ambiguous`. No matching filename produces `parser.not_found`; a matching
GitHub entry without a digest produces `parser.digest_missing`. These errors have
`{ name }`, preserving the original artifact name. Non-header rules check body size
first.

## Tests

The conformance runner shares `run_cases` and result comparison between version
and checksum cases, collects all failing names, and compares exact codes, full
fields, and paths. A separate libtest recursively checks global corpus-name
uniqueness, excluding fixture expected lists.

```sh
cargo test -p manifest-parser
cargo test -p manifest-parser --test conformance -- --nocapture
```

See the [corpus contract](../../conformance/README.md) and
[Rust guide](../../docs/development/rust.md).
