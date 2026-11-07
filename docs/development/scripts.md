# Repository scripts

Repository automation is TypeScript run directly on Node.js >= 24.2 with native
type stripping. There is no script build step. The root `package.json` is the only
task catalog; list commands with `pnpm run` and run public tasks from the repository
root as `pnpm run <name>`.

## Layout and responsibilities

| Directory under `scripts/src/` | Responsibility                                                                  |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `wasm/`                        | Locked Rust wasm build, wasm-bindgen version check, and runtime glue generation |
| `conformance/`                 | Rust/Node runner orchestration and executed case-name parity                    |
| `generate/`                    | Generated-output Git cleanliness checks                                         |
| `shared/`                      | Small shared modules, currently source-relative repository paths                |

Keep handwritten source under these responsibility directories. Keep
[`scripts/README.md`](../../scripts/README.md) as the task table; the Rust schema
generator stays in its owning crate. Scripts currently use only `node:` built-ins,
so no separate scripts workspace package or dependency manifest is needed. Root
dev dependencies provide formatting, lint, and Node types; the root TypeScript
configuration includes `scripts/src/**/*.ts`.

Keep orchestration, computation, and infrastructure responsibilities clear. Do not
add broad `utils/` buckets, old-path wrappers, compatibility exports, or a framework
to avoid updating callers. A reusable module import must not launch a CLI or start
work. If script tests become necessary, co-locate `*_test.ts` with their modules
and add a named task for running them.

## Task entrypoints

Use named root tasks in CI, active documentation, and tooling callers. Use
`pnpm run`, including inside task composition, because bare task names can collide
with pnpm built-ins. A TypeScript CLI task points to
`node scripts/src/<category>/<file>.ts`; keep execution logic in that file.
Where pnpm cannot be assumed on a spawned process's PATH, use
`node --run <name> -- <args>`.

The existing `generate:schema` task invokes the Rust generator and Prettier;
`check:generated` composes it with the Git check. `test` and `check:conformance`
compose the wasm build with their runners. Do not duplicate implementation in
shell snippets merely to reorganize paths. Historical plans retain the commands
they originally recorded.

```sh
pnpm run build:wasm
pnpm run check:conformance
pnpm run check:generated
```

Use `shared/repo-paths.ts` for source-relative repository paths instead of counting
parents independently in each moved file. CLI path arguments are resolved from the
invocation directory; run tasks at the root. A task that intentionally targets a
different workspace must accept an explicit path rather than changing shared state.
Preserve arguments, environment, permissions, and failure exit status when moving
scripts. Never embed credentials in task definitions.

`build:wasm` gets Cargo's target directory from `cargo metadata`, respecting
`CARGO_TARGET_DIR`, checks the CLI against the lockfile, and runs Cargo with
`--locked`. Its generated glue belongs in ignored
`packages/manifest-runtime/wasm/`. Each worktree must still own its writable target.
The generated-file check uses Git porcelain status including untracked files for
the whole `packages/manifest-metadata` package and
`packages/manifest-runtime/src/types.ts`, so commit any other change in those
paths (including the metadata README) before running it.

## Dependencies and verification

Import Node APIs with `node:` and local modules with `.ts`. Use erasable syntax;
enums, namespaces, and constructor parameter properties are prohibited. Do not
install a script dependency unless the task requires it. If dependencies become
necessary, declare and lock them deliberately rather than relying on transitive
packages.

Do not use native Windows paths as glob patterns: backslashes can be interpreted
as escapes. Pass exact paths or use relative globs with an explicit working
directory. Normalize separators in cross-platform path comparisons.

Prettier, Oxlint, and the shared `tsc --noEmit` check script sources. For a layout
change, verify root-relative paths, argument forwarding, inherited environment,
generated outputs, and the actual task entrypoints:

```sh
pnpm run check
pnpm run test
pnpm run check:conformance
pnpm run check:generated
```

These also exercise `fmt:check`, `lint`, `typecheck`, `build:wasm`, and
`generate:schema`. Apply formatting with `pnpm run fmt`. The
[testing guide](testing.md) records toolchain requirements and the conformance
runner contract. Report any platform check that could not run.
