# resources-manifest

A resource manifest v2 workspace with Rust metadata, a shared semantic core, and
Node.js packages for generation and build-time downloads. Rust defines the wire
types and generates TypeScript definitions, version constants, and JSON Schema.
Native Rust hosts use `manifest-core`; Node hosts call the same semantics through
`@manifest/runtime` and WebAssembly.

`@manifest/generator` captures upstream releases into a snapshot and compiles the
manifest for the cores clash-nyanpasu ships through the same core. Signing and the
retirement of the upstream scripts are recorded in the design documents.

## Workspace layout

| Path                               | Responsibility                                                                                                                                                                                          |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/manifest-parser`           | Checksum/parser wire types and pure implementations, `ResourceChecksum`, limits, responses, and parser errors; no IO/async dependencies.                                                                |
| `crates/manifest-metadata`         | Manifest/template types annotated with Serde, Schemars, and Specta; validated manifest versions and finite numbers. Depends on parser.                                                                  |
| `crates/manifest-core`             | Pure compilation, semantic validation, validated manifests, templates, variant selection, version/download plans, checksum kinds, and policy. Depends on metadata and parser; no IO/async dependencies. |
| `crates/manifest-wasm`             | Thin wasm-bindgen adapter for the Rust semantics.                                                                                                                                                       |
| `crates/manifest-schema-generator` | Generates metadata TS types, version constants, JSON schemas, and runtime plan types.                                                                                                                   |
| `packages/manifest-metadata`       | Pure TypeScript structural contracts and generated schemas; see its [contract guide](packages/manifest-metadata/README.md).                                                                             |
| `packages/manifest-runtime`        | Node and browser facade loading wasm and exposing shared semantics and opaque handles.                                                                                                                  |
| `packages/manifest-generator`      | Node CLI capturing GitHub releases, observing alpha versions through the manifest's own plans, and compiling through runtime; HTTP in `http.ts`.                                                        |
| `packages/manifest-downloader`     | Node CLI selecting, planning, and verifying downloads through runtime; HTTP in `http.ts`, streaming temporary files, verified atomic output.                                                            |
| `packages/manifest-viewer`         | Astro and Svelte page inspecting a manifest in the browser through runtime; deployed to GitHub Pages.                                                                                                   |
| `scripts/src/`                     | Repository automation grouped by responsibility; see the [task table](scripts/README.md).                                                                                                               |
| `conformance/`                     | Language-neutral corpus shared by Rust and Node, plus fixed HTML fixtures and reference goldens.                                                                                                        |
| `docs/development/`                | Current development rules for contributors and agents.                                                                                                                                                  |
| `docs/spec/`, `docs/design/`       | Historical specifications, normative protocol contracts, and design decisions.                                                                                                                          |
| `files/`                           | Read-only upstream implementation/design references, untracked and excluded from lint, formatting, and tests.                                                                                           |

Root `Cargo.toml` declares `crates/*` and centralizes dependency versions in
`[workspace.dependencies]`. Root `pnpm-workspace.yaml` declares `packages/*`;
cross-package dependencies use package names and `workspace:*`. Root `tsconfig.json`
checks all packages except the viewer, and scripts, with `module: nodenext`; the
viewer is checked by its own `svelte-check` under `pnpm run typecheck`.

## Requirements

- Node.js >= 24.2, using native TypeScript type stripping and `import.meta.main`.
- pnpm at the version pinned by root `package.json` (`pnpm@12.17.0`).
- A Rust toolchain supporting edition 2024 and the locked dependencies. There is
  currently no `rust-toolchain.toml`; prescribed verification uses Rust 1.95.0.
- The selected toolchain's `wasm32-unknown-unknown` target and a wasm-bindgen CLI
  matching `Cargo.lock` (currently 0.2.114). The build task checks this match.

TypeScript must use erasable syntax and `.ts` relative imports. Packages export
source without a build step. Node does not strip types from `.ts` in `node_modules`,
so these packages currently support workspace use; publishing needs a build step.
Runtime's main entry targets Node build scripts; its `@manifest/runtime/web` entry
serves pages bundled by Vite, such as the viewer. Tauri webviews are not supported.

## Get started

Run from the repository root:

```sh
pnpm install
rustup target add wasm32-unknown-unknown --toolchain 1.95.0
cargo +1.95.0 install wasm-bindgen-cli --version 0.2.114 --locked
```

Use the verification environment in PowerShell:

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

`test` builds wasm before running `packages/*/*_test.ts`, including Node
conformance. `check:conformance` builds wasm and compares the four Rust corpus
runners' successful case names with Node's. Use `pnpm run build:wasm` before
importing runtime outside those tasks. See the [downloader README](packages/manifest-downloader/README.md)
for CLI usage.

After changing Rust metadata or exported plan types, run
`pnpm run generate:schema` and include generated outputs with the change.
`pnpm run check:generated` must report no drift, including untracked files.
Never hand-edit metadata `src/types.ts`, `src/version.ts`, `schema/`, or runtime
`src/types.ts`. `pnpm run fmt` applies Prettier; `pnpm run check` combines its
format check with Oxlint and `tsc --noEmit`; the type check builds wasm first.

The tracked real-manifest snapshot at
`crates/manifest-metadata/tests/fixtures/resources.json` is shared by Rust and
Node and excluded from Prettier to retain its original bytes.

## Development

Read the [development standards](docs/development/README.md) before making changes:

- [Rust APIs, crate layering, and wire contracts](docs/development/rust.md)
- [TypeScript and Node conventions](docs/development/typescript.md)
- [Testing, conformance, and review](docs/development/testing.md)
- [Workflow, worktrees, host toolchain caveat, and commits](docs/development/workflow.md)
- [Repository scripts](docs/development/scripts.md)

Rust checks are `cargo test --workspace`,
`cargo clippy --workspace --all-targets`, and `cargo fmt --all -- --check`.
Keep build outputs on `G:` on this Windows development host; see the workflow's
ReFS compression caveat if toolchain reads fail.

## License

GPL-3.0-only, as declared by the workspace manifests.
