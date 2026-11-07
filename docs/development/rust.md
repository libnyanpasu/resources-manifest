# Rust code guide

This guide applies to handwritten Rust in the root Cargo workspace (`crates/*`).
It adapts the clash-nyanpasu Rust guide and adopts the
[Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html)
where applicable, with the project decisions below. Protocol requirements remain
defined by the [core/parser design](../spec/2026-11-06-manifest-core-parsers/design.md)
§5 and §8 and the [manifest v2 design](../design/manifest-v2.md).

Generated artifacts follow their generators. `files/` is read-only upstream
reference material, outside formatting, lint, and tests.

## 1. Rule status

- "Must" and "do not" are requirements. "Prefer" and "should" are reviewable
  defaults; state the reason for a deviation.
- New code must follow this guide. Existing violations are migration debt, not
  examples to copy. Do not fix unrelated code as part of another task.
- Listed deviations from Google are deliberate. Propose further shared exceptions
  by updating this guide, rather than inventing local rules.
- Configuration-dependent rules are adopted only where the configuration exists.
  Documentation does not silently change the toolchain or lint severity.

## 2. Deviations from Google

| Google guidance                                                                              | Project decision                                                                                                                                                                                  |
| -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Prefer small crates and flat `rust_library` layout [chrome]                                  | Use conventional Cargo `crates/*`; split by ownership and dependency boundaries.                                                                                                                  |
| Import at module granularity                                                                 | Group imports logically and let current stable rustfmt defaults format them. The reference's `imports_granularity = 'Crate'` setting is not adopted because no rustfmt configuration exists here. |
| Do not implement APIs in non-leaf modules                                                    | Existing crate roots own central public types and small entrypoints. Keep substantial computations in focused modules; do not reorganize roots solely for style.                                  |
| Platform-specific async, threading, synchronization, and gtest restrictions [chrome/Fuchsia] | Parser and core are synchronous and have no IO/async dependencies. Rust tests use `#[test]`; host IO belongs outside these crates. No application runtime is prescribed.                          |
| Prefer generics over `dyn Trait`                                                             | Prefer concrete computation; add generic or dynamic dispatch only for actual consumers or a narrow host boundary. No actor or dependency-injection framework is required.                         |

Other applicable Google rules retain their own exceptions, including deliberate
shared ownership, owned error payloads, and `Vec` returns for naturally built
collections. The reference's actors, Tauri adapters, panic framework, and runtime
crates are not part of this repository.

## 3. Formatting, lints, and checks

### Toolchain and rustfmt

The workspace uses Rust 2024. There is currently no `rust-toolchain.toml` or
`rustfmt.toml`; no repository file pins the Rust toolchain. Use rustfmt defaults
with the selected toolchain, and preserve LF line endings. The reference's nightly
selection, crate-granularity import grouping, derive merging, and other custom
rustfmt settings have not been adopted. Do not add configuration or unstable
features merely to reproduce the reference.

Use the prescribed Rust 1.95.0 environment when verifying this work; see
[Host toolchain caveat](workflow.md#host-toolchain-caveat). Rust files are outside
Prettier's language support. Let rustfmt format expressions, imports, attributes,
and `where` clauses; reserve `#[rustfmt::skip]` for justified manual alignment or
generated output.

### Clippy and suppressions

There is no `clippy.toml` or workspace lint table today. Run
`cargo clippy --workspace --all-targets`; do not add warnings. Suppress a lint only
at the narrowest necessary scope with a reason. Prefer `#[expect(..., reason =
"...")]` when applicable. Do not introduce blanket allow lists or warning policy
changes in unrelated work.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

For wire-type changes, also run `pnpm run generate:schema`,
`pnpm run check:generated`, and the relevant cross-language checks in
[Testing and review](testing.md). Commit generated outputs with their source
changes when commits are requested.

## 4. Modules, imports, and crate layering

- Give each leaf module one responsibility and a concrete name such as `checksum`,
  `response`, `limits`, `render`, `plan`, or `verify`.
- Keep module declarations, re-exports, and small entrypoints readable. Move
  substantial implementations to their owning modules when the task calls for it.
- Use the narrowest useful visibility: private, then `pub(super)` or `pub(crate)`,
  then `pub` for actual consumers.
- Avoid glob imports. Google's exceptions are inline `use super::*`, enum-local
  `use Enum::*`, and a non-leaf re-export module. Do not expand framework preludes
  or rewrite existing code outside the task solely for style.
- Import types and use them unqualified; keep module paths when they add context
  or avoid collisions (`io::Error`, `fmt::Result`). Qualify non-local free functions
  through their module (`serde_json::to_string`).
- Put imports at the top of the file or function, grouped as standard library,
  external/workspace crates, then `crate`/`super`/`self`. Import traits where used;
  use `Trait as _` when only its methods are needed.
- Update callers when moving an item. Do not leave old-path compatibility
  re-exports to avoid fixing imports.

Dependency arrows below mean "depends on":

```text
manifest-metadata -> manifest-parser
manifest-core -> manifest-metadata + manifest-parser
manifest-wasm -> manifest-core + manifest-metadata + manifest-parser
manifest-schema-generator -> manifest-metadata + manifest-core
```

`manifest-parser` owns checksum/parser wire types and their pure implementations,
`ResourceChecksum`, `VersionLabel`, `Limits`, `RemoteResponse`, and `ParseError`.
It must not depend on metadata or core. Metadata owns manifest/template DTOs,
`ManifestVersion`, and `FiniteF64`; it must not depend on core. Core owns pure
semantic validation, `ValidatedManifest`, templates, variant selection, version
and download plans, `ChecksumKind`, and `VerifyPolicy`.

Neither parser nor core may introduce filesystem, HTTP, or async dependencies.
Wasm is a thin adaptation layer; the schema generator owns output generation.
Dependency versions belong in root `[workspace.dependencies]` and are consumed
with `workspace = true`. Check dependency changes with `cargo tree`.

## 5. Function APIs, types, and traits

### 5.1 Library and application code

- Distinguish reusable library contracts from generator/binary orchestration.
  Do not add speculative builders, traits, generics, or compatibility APIs.

### 5.2 Inputs, outputs, and data types

- Accept flexible borrows (`&str`, `&[T]`, `&T`, `Option<&T>`, `&Path`) instead of
  borrowing owning containers. Take ownership when storing or consuming a value.
- Prefer unique ownership. Use shared ownership for an actual shared immutable
  lifetime, such as wasm selections retaining a manifest; do not clone deeply just
  to silence the borrow checker.
- Use newtypes for distinct meanings and validated invariants. Keep fields private
  when construction establishes an invariant, and route decoding through validation.
- `ManifestVersion` accepts major 2 and a canonical, non-leading-zero `u32` minor,
  such as `"2.0"`. `ResourceChecksum` validates and normalizes `<algorithm>:<hex>`.
  `FiniteF64` rejects NaN and infinities. Do not weaken these into unchecked aliases.
- Prefer named fields to multi-field tuples, enums to unclear boolean arguments,
  and `Option` to sentinel values. A clear serialized binary setting may stay bool.

### 5.3 Traits

- Implement standard traits (`FromStr`, `Display`, `From`, `TryFrom`) when their
  semantics fit. Derive only traits whose behavior is meaningful. Pair `len()`
  with `is_empty()` and options structs with an appropriate `Default`.
- Introduce traits for real generic consumers, extension methods, or narrow
  injected boundaries. Do not define a trait merely to group functions.

### 5.4 Shared ownership

Prefer borrowing and unique ownership. Share immutable values only when their
consumers require independent lifetimes; keep mutation and cleanup ownership clear.

### 5.5 Attributes

- Use `#[non_exhaustive]` for an explicit evolution promise, not automatically on
  every internal DTO. Preserve existing public compatibility commitments.
- Use `#[must_use]` where discarding a pure result, plan, or guard is a bug.
  `Result` already carries it. Make public functions `const` only if that can remain
  part of the contract.

## 6. Configuration and wire types

- Define a type once in its owning crate. Metadata and parser use Serde,
  Schemars, and Specta annotations; core exposes plan/error types through the
  existing Specta generator. Use `serde-wasm-bindgen` at the wasm boundary,
  without a second exporter or copied domain structs.
- Serialized names and enum tags are contracts. Preserve explicit Serde names;
  `ChecksumKind` uses `authoritative`, `parsed`, `snapshot`, `none`, while the
  existing `github-release-assets` tag remains kebab-case.
- Preserve compiled-manifest compatibility: accept valid same-major minors,
  expose `newer_minor`, skip unknown platform keys, and isolate unknown parser tags
  to the affected artifact with `resolve.unsupported_parser`. Unknown parser kinds
  are `none`. Do not reject an entire newer minor solely against an older schema.
- Keep template-only decoding strict and compiled decoding permissive according to
  the existing type boundaries. Shared DTOs are not all strict. Optional Rust
  fields accept explicit null as `None`; generated TS/schema properties permit
  absence or a concrete value. See the [metadata contract](../../packages/manifest-metadata/README.md).
- Keep deterministic ordering, exact tag sets, and all cross-field invariants in
  core. Schema shape validation does not replace `validate_manifest`.
- Keep authored template values recursive and host/parser literals non-recursive.
  Validate host version/source values as `VersionLabel`; preserve builtin variable
  precedence, cycle detection, URL safety, and byte-for-byte reference behavior.
- `Limits` has private fields and lower-only setters. Never deserialize directly
  into an unchecked limit or allow a host to raise defaults.
- Preserve checksum provenance and policy: embedded static release checksums are
  authoritative; declared remote parsers produce parsed checksums; snapshot is an
  explicit exact-version fallback. No policy accepts `none`. Mirrors only transport.
- Generated `packages/manifest-metadata/src/{types,version}.ts`, its `schema/`, and
  `packages/manifest-runtime/src/types.ts` must be regenerated, never hand-edited.

## 7. Errors, panics, and assertions

### 7.1 Choosing the mechanism

- Use `Option` for ordinary absence, `Result` for recoverable parsing, validation,
  planning, or host failures, and panic only for a programming invariant. Untrusted
  manifests, responses, and host arguments must not cause intentional panics.

### 7.2 Error types

- Use typed errors with structured fields; follow the existing Snafu conventions.
  Do not replace actionable variants with `Custom(String)` or parse display text.
  Name variants after the failing step and retain actionable domain information.
- Preserve stable `code()`, complete `fields()`, and exact `path()` across layers.
  Paths use dotted document locators and `[i]` indices, `arg.<name>` for host input,
  `response.body` or `response.headers.<lowercase-name>` for parser responses, and
  `parser.pattern`, `parser.group`, or `parser.type` for rule errors. Keep all actual
  wire nesting when prefixing a rule path during manifest validation.
- Wasm errors are `{ code, path, message, fields }`; the Node facade wraps structured
  errors as `ManifestRuntimeError` and rethrows unrelated exceptions. Preserve the
  validation aggregate's ordered original errors at that boundary.
- Preserve underlying error sources where useful. Use `?`, `match`, `if let`, or
  `let ... else` rather than checking then unwrapping.
- In production, prefer `expect()` with the invariant's reason over bare `unwrap()`.
  Tests may unwrap. Add useful assertion context when a failure is otherwise opaque.

### 7.3 Panic recovery and poisoned locks

- Do not use `catch_unwind` for ordinary error handling or recover poisoned locks
  to continue with potentially inconsistent state. Tests may assert panic behavior.

The complete error tables and path/field contracts live in design §5.6 and the
[conformance README](../../conformance/README.md).

## 8. Ownership and execution boundaries

Keep parser/core computation synchronous and independent of execution runtimes.
Hosts perform requests, enforce transport limits, calculate file digests, and
write output. Downloader HTTP belongs in `packages/manifest-downloader/http.ts`;
artifacts stream into temporary files and move atomically only after verification.
Do not duplicate core semantics in TypeScript or add ambient mutable state to pure
APIs. Wasm handles retain the state they need and must be released by their host.

## 9. Macros, unsafe code, and platform code

- Prefer functions, types, and traits before macros. Use existing derives and
  generators instead of handwritten generated output.
- Use unsafe only when safe Rust cannot express the requirement, behind a small
  safe boundary. Each unsafe block/impl needs a `// SAFETY:` justification; public
  unsafe APIs document `# Safety`. Do not add `unsafe impl Send`/`Sync` to bypass
  ownership requirements.
- Do not mutate process environment in library computation. Inject host inputs.
- Gate platform-specific items explicitly and keep counterpart signatures aligned.
  Keep test-only APIs behind `#[cfg(test)]` or in test-support modules.

## 10. Naming and documentation

- Use `snake_case` for functions, modules, fields, and locals; `PascalCase` for types,
  traits, and variants; `SCREAMING_SNAKE_CASE` for constants.
- Name computations and domain concepts directly; avoid `Manager`, `Helper`, and
  `Utils`. Keep established protocol terms and avoid opaque abbreviations.
- Use `as_` for cheap borrowed views, `to_` for owned/expensive conversions, and
  `into_` for consuming conversions. Use `new`, `new_with_foo`, `try_new`, and
  `from_foo` consistently; use `parse` for untrusted text.
- Accessors use `foo`, `foo_mut`, `set_foo`, `take_foo`, or `replace_foo`, without
  redundant `get_`. Use descriptive lifetimes when several borrows interact.
- Document exported cross-crate APIs: ownership, units, ordering, limits, and failure
  contracts. Use `///` for items and `//!` for modules/crates; explain constraints,
  not the item name. Add `# Errors`, `# Panics`, and `# Safety` where applicable.
- Implementation comments explain non-obvious invariants. Do not commit
  commented-out code or change journals.

## 11. Implementation readability and tests

Annotate bindings when inference hides the intent of `parse`, `into`, or `collect`.
Prefer short iterator chains for transformations and loops for multi-step effects
or early returns; avoid `for_each` for effects. Use `format!`/`write!` for
substitution. Keep diagnostics at host boundaries, without exposing credentials
or changing machine-readable errors into log text.

Test pure functions with plain values, public contracts through integration tests,
and shared semantics through the corpus. Match meaningful codes, fields, and paths
rather than only `is_err()`. Use temporary paths and local-only HTTP fixtures for
host tests. See [Testing and review](testing.md).

## 12. Migration debt

Existing style differences are not permission for unrelated rewrites. Handle
constructor naming, module layout, documentation gaps, lint suppressions, or
unchecked fallible paths in focused changes with behavior verification. This
guide adds no nightly selection, custom formatter configuration, new dependency,
or lint configuration. Those require their own deliberate adoption.

## 13. Review checklist

- Formatting, Clippy, and relevant behavior checks pass.
- Each changed line belongs to the request; new code follows the guide.
- Crate dependency direction holds; parser/core remain free of IO/async.
- Types retain their invariants and generated outputs match their Rust sources.
- Wire names, compatibility, checksum provenance, and policy remain intact.
- Errors preserve codes, full fields, paths, and meaningful sources.
- Unsafe code and lint suppressions are narrow and justified; input errors cannot
  panic, and no poisoned lock is recovered.
- Tests cover observable behavior and Rust/Node conformance remains equal.
