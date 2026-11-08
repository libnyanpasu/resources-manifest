# AGENTS.md

This workspace defines resource manifest v2 types in Rust and generates/consumes
its contracts through Node.js packages. Keep protocol semantics shared through the
pure Rust core and its wasm facade.

## Mandatory Reading: Development Standards

Before repository work, agents MUST read [docs/development/README.md](docs/development/README.md)
and all guides: [Rust](docs/development/rust.md),
[TypeScript](docs/development/typescript.md),
[testing and review](docs/development/testing.md),
[workflow](docs/development/workflow.md), and
[repository scripts](docs/development/scripts.md).

These guides are mandatory project requirements, not optional background. Reading
this file alone is insufficient. Resolve unavailable or conflicting requirements
before affected work. Use the [root README](README.md) for layout and setup and
package READMEs for their contracts.

## 0. Synchronization Policy

`CLAUDE.md` imports this file so both tools follow the same rules. Detailed shared
rules live in `docs/development/`; this file routes to them and restates only the
most frequently violated requirements.

- Update a guide when its shared rule changes. Update this file in the same change
  only if it restates that rule.
- Link to guides instead of copying their sections.
- Do not add tool-specific exceptions unless a tool actually requires them.
- Preserve historical/normative records in `docs/spec/` and `docs/design/`; follow
  the development index's distinction between current workflow and protocol records.

## 1. Think Before Coding

**State assumptions and make tradeoffs visible.**

Identify material ambiguity before dependent implementation. Explain competing
interpretations and simpler alternatives when they affect the result. For small,
obvious tasks, keep this brief; for architecture or cross-package work, be explicit.
Follow choices the user has already made.

## 2. Simplicity First

**Write the minimum complete solution.**

- Do not add unrequested features, speculative abstractions, or configurability.
- Do not handle impossible scenarios or introduce dependencies without need.
- Simplify needless complexity while preserving architecture and type invariants.

## 3. Surgical Changes

**Every changed line must trace to the request.**

- Do not refactor adjacent code, comments, or formatting outside the task.
- Match local layout; new code still follows the language guides.
- Remove imports, variables, and functions made unused by your own change.
- Mention unrelated dead code rather than deleting it.
- Keep `files/` read-only and outside lint, formatting, and tests.

## 4. Goal-Driven Execution

**Define success criteria and verify them.**

Turn tasks into observable goals. Reproduce a bug before fixing it; verify behavior
before and after refactors. Pair each step of multi-step work with a check, run
relevant checks, and report exact limitations. Do not treat a fallback toolchain
as validation on the prescribed one.

## 5. Architecture

Read [Rust crate layering and wire contracts](docs/development/rust.md#4-modules-imports-and-crate-layering),
[TypeScript host boundaries](docs/development/typescript.md#errors-and-host-boundaries),
and the normative [core/parser design](docs/spec/2026-11-06-manifest-core-parsers/design.md)
§5 and §8 before changing those boundaries.

- Metadata depends on parser; core depends on metadata and parser. Parser must not
  depend on metadata/core, and metadata must not depend on core. Parser/core have
  no IO or async dependencies.
- Each type has one owner. Preserve validating `ManifestVersion`, `ResourceChecksum`,
  `FiniteF64`, and lower-only `Limits` invariants.
- Keep semantic validation, templates, selection, plans, kinds, and policy in core.
  Wasm stays a thin adapter; Node hosts use `@manifest/runtime`.
- Generated metadata types, version constants, schemas, and runtime types must not
  be hand-edited. Run `pnpm run generate:schema` after their Rust sources change;
  include generated outputs, and require `pnpm run check:generated` to be clean.
- Preserve same-major compatibility, stable error codes/full fields/exact paths,
  checksum provenance, and explicit policy. No policy accepts `none`; mirrors do
  not change trust, and undeclared headers cannot supply checksums.
- Use public package names and `workspace:*`, `.ts` relative imports, and erasable
  syntax. Keep downloader HTTP in `http.ts`; stream temporary artifacts and verify
  before atomic replacement.

## 6. Testing

Follow [Testing and review](docs/development/testing.md). Test pure computation with
plain values and host effects with local fixtures. Preserve the shared snapshot
and exact Rust/Node conformance case parity. The guide lists commands, runners,
error comparisons, and required Rust/wasm prerequisites.

## 7. Code Style and Repository Conventions

| Area                                                        | Guide                                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Rust formatting, APIs, layering, wire types, errors, panics | [Rust code guide](docs/development/rust.md)                                           |
| Node runtime, imports, constants, grouping, generated files | [TypeScript code style](docs/development/typescript.md)                               |
| Automation layout and root task catalog                     | [Repository scripts](docs/development/scripts.md) and [task table](scripts/README.md) |
| Verification, fixture ownership, and final review           | [Testing and review](docs/development/testing.md)                                     |
| Worktrees, host caveat, and commits                         | [Development workflow](docs/development/workflow.md)                                  |

Apply guides when planning and implementing, not only after lint passes. Existing
violations are migration debt; do not copy or fix them outside the requested scope.

## 8. Worktrees

Follow [Worktrees and resource reuse](docs/development/workflow.md#worktrees-and-resource-reuse).
Assess costs before selecting a worktree or current checkout, and honor an existing
user choice. On Windows ReFS, prefer the same `G:` volume; reuse target data only
through verified native CoW with independent writable files. Never share targets
through links or a common output directory, and never put build output on `C:`.
Read the [host toolchain caveat](docs/development/workflow.md#host-toolchain-caveat)
before investigating rustc ICEs or `LNK1190`.

## 9. Git Commits

Follow [Git commits](docs/development/workflow.md#git-commits).

- Stage explicit related paths only; never `git add .`, `-A`, `--all`, or `*`.
- One commit does one thing and builds. Fold corrections into the unpushed commit
  they complete; do not make fix-up commits. Rewriting pushed history needs consent.
- Use an imperative subject of at most 72 characters without a trailing period.
  Explain why in a non-trivial body and trust the reader.
- Follow the user's delivery instructions, including requests not to commit or push.

## 10. Final Review

Walk through [Review before submitting](docs/development/testing.md#review-before-submitting).
Check scope, crate boundaries, wire/error compatibility, generated outputs, and
behavior verification. Report what changed, which checks passed, and any remaining
limitations. Leave unrelated files and user changes untouched.
