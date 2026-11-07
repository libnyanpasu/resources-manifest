# Testing and review

## Design tests around boundaries

- Test parser/core computation directly with plain values. Exercise public APIs
  through Rust integration tests instead of introducing IO or an async runtime.
- Test observable success, rejection, ordering, compatibility, and resource limits.
  Cover invalid inputs and exact boundary values, including UTF-8 byte counts.
- Assert stable error codes, complete structured fields, and exact paths. Do not
  parse message text or settle for `is_err()` where the failure contract matters.
- Test wasm/Node conversion, opaque-handle ownership and disposal, and error
  translation at the facade boundary. Generated structural types do not establish
  semantic validity.
- Use temporary paths and local-only HTTP fixtures for downloader tests. Servers
  bind to `127.0.0.1` on ephemeral ports; test directories are inside the package.
  Tests must not contact live upstream services or use real user output files.
- Coordinate fixtures through explicit lifecycle signals. Do not use global mutable
  fixtures or arbitrary sleeps as synchronization.
- Reproduce a behavior bug before fixing it. Add tests for meaningful contract
  changes rather than mirroring implementation branches or testing prose edits.

## Conformance is the cross-language contract

[`conformance/`](../../conformance/README.md) contains language-neutral inputs and
expected results. Preserve input bytes, duplicate headers, and array order.
Compare JSON structure, stable error codes, full fields, and exact locators;
validation errors are compared as a multiset, including duplicate counts.

| Runner                                           | Responsibility                                                                              |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `crates/manifest-parser/tests/conformance.rs`    | Version/checksum parsers, GitHub HTML fixtures, and recursive global corpus-name uniqueness |
| `crates/manifest-core/tests/conformance.rs`      | Validate, compatibility, checksum kinds, selection, plans, and policy                       |
| `crates/manifest-core/tests/resolve.rs`          | Template/artifact rendering and snapshot URL regressions                                    |
| `crates/manifest-core/tests/reference_golden.rs` | Fixed upstream reference results for all default resource/platform combinations             |
| `packages/manifest-runtime/conformance_test.ts`  | The same cases through Node and wasm, one test per corpus case or fixture asset             |

`pnpm run check:conformance` first builds wasm, then executes these four Rust
runners and the Node runner. `MANIFEST_CONFORMANCE_LIST` records only successfully
asserted case names into separate temporary JSONL files. The task rejects child
failures, empty lists, duplicate names, and either direction of set difference;
it prints per-directory/prefix counts and totals. Fixture assets participate
individually. Temporary lists are cleaned after success or failure.

The tracked real-manifest snapshot is
`crates/manifest-metadata/tests/fixtures/resources.json`, shared by Rust and Node.
It is tracked because `files/` is an untracked read-only reference directory;
`.prettierignore` preserves the snapshot's original bytes. Tests use the checked-in
snapshot/goldens and fixed HTML expected lists, not runtime expectations computed
by the implementation under test. Reference material in `files/` does not
participate in lint, formatting, or normal tests.

## Run the relevant checks

Install workspace dependencies with `pnpm install`. Wasm checks require
`wasm32-unknown-unknown` for the selected toolchain and a wasm-bindgen CLI matching
`Cargo.lock`. `pnpm run test` and `pnpm run check:conformance` build wasm themselves,
so a fresh checkout does not need a separate manual build first. Type checking
can run before wasm generation.

| Check                                             | Command                                  |
| ------------------------------------------------- | ---------------------------------------- |
| Formatting, Oxlint, and TypeScript                | `pnpm run check`                         |
| Apply Prettier formatting                         | `pnpm run fmt`                           |
| Node package tests, including runtime conformance | `pnpm run test`                          |
| Rust/Node corpus execution parity                 | `pnpm run check:conformance`             |
| Regenerate types, versions, and schemas           | `pnpm run generate:schema`               |
| Generated drift, including untracked outputs      | `pnpm run check:generated`               |
| Rust formatting                                   | `cargo fmt --all -- --check`             |
| Rust workspace tests                              | `cargo test --workspace`                 |
| Rust static checks                                | `cargo clippy --workspace --all-targets` |

For focused work, select the affected Cargo package/integration test or Node
`*_test.ts` file, then complete the checks required by the task. For a scripts
reorganization, run all public task paths and verify generated outputs and case
parity. Avoid concurrent heavy builds; limit Cargo jobs when resources are tight.

Use the prescribed verification environment in PowerShell:

```powershell
$env:RUSTUP_TOOLCHAIN = "1.95.0"
$env:CARGO_BUILD_RUSTC_WRAPPER = ""
$env:CARGO_INCREMENTAL = "0"
$env:CARGO_BUILD_JOBS = "2"
pnpm run check
pnpm run test
pnpm run check:conformance
pnpm run check:generated
```

See [Host toolchain caveat](workflow.md#host-toolchain-caveat) before treating an
ICE or linker failure as a source regression. Report environment-dependent checks
that could not run; a fallback toolchain is not proof that the prescribed one passed.

## Review before submitting

- Every changed line supports the request; assumptions and success criteria are clear.
- Dependency direction holds, and parser/core remain free of IO and async dependencies.
- Wire types have one owner, retain their invariants, and preserve compatibility.
- Generated outputs are current; schemas describe structure without replacing
  semantic validation.
- Checksum provenance, effective policy, explicit header sources, template literal
  behavior, and URL constraints remain intact.
- Error codes, paths, and full fields survive native, wasm, and Node boundaries.
- Tests exercise behavior with pure values or boundary fixtures; Rust and Node
  execute the same named conformance cases.
- Relevant checks pass, and any limitation is documented precisely.
- Language style follows the [Rust](rust.md) or [TypeScript](typescript.md) guide;
  names, grouping, and reuse have been reviewed as well as mechanical formatting.
- Scripts follow the [repository layout](scripts.md), and all active callers use
  the named tasks.
- Only related paths are staged when committing; the commit is atomic and buildable.
