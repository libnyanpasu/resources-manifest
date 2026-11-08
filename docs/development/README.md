# Development standards

These are the current development rules for human contributors and coding agents.
They are the single source of detailed development rules; [AGENTS.md](../../AGENTS.md)
routes agents here and restates the most frequently violated requirements.

## Choose a guide

| When you are working on                                             | Read                                   |
| ------------------------------------------------------------------- | -------------------------------------- |
| Crate boundaries, Rust APIs, wire types, errors, or panics          | [Rust code guide](rust.md)             |
| Node.js packages, TypeScript modules, constants, or generated files | [TypeScript code style](typescript.md) |
| Tests, conformance, verification, or final review                   | [Testing and review](testing.md)       |
| Scope, worktrees, host toolchain problems, or commits               | [Development workflow](workflow.md)    |
| Repository automation, task entrypoints, or script layout           | [Repository scripts](scripts.md)       |

Start with the [root README](../../README.md) for workspace layout, requirements,
and setup. Package READMEs document their public surfaces and contract boundaries.

## Rules to keep in mind

- Keep parser independent of metadata and core. Metadata depends on parser; core
  depends on both. Neither parser nor core may introduce IO or async dependencies.
- Define each wire type once in its owning crate. Use Serde, Schemars, and Specta
  through the existing generator; never hand-edit generated types or schemas.
- Keep semantic decisions in core and HTTP/filesystem effects in hosts. Node uses
  `@manifest/runtime`; native Rust hosts depend on `manifest-core` directly.
- Preserve stable error codes, complete structured fields, and exact paths. Message
  text is for people, not programmatic matching.
- Preserve checksum trust kinds and explicit verification policy. Mirrors do not
  change trust, and undeclared response headers cannot supply checksums.
- Write to the [Rust](rust.md) and [TypeScript](typescript.md) guides even when
  nearby code predates them. Keep unrelated cleanup in separate changes.
- Keep automation in `scripts/src/` and use named `pnpm run` tasks.
- Verify behavior at boundaries and keep the Rust/Node conformance case sets equal.
- Prefer isolated worktrees for feature/refactoring work, while respecting the
  user's chosen checkout. Each worktree owns its writable build outputs.
- Keep changes focused and commits atomic.

## Maintaining these rules

Update the relevant guide when a shared rule changes. Update `AGENTS.md` in the
same change only if it restates that rule; do not duplicate whole guide sections.
`CLAUDE.md` imports `AGENTS.md`, so the tools share the same rules. Do not add
tool-specific architectural exceptions without an actual tool requirement.

`docs/spec/` and `docs/design/` contain historical proposals, normative contracts,
and recorded decisions. Preserve their language and historical commands. Use these
development guides for current coding and workflow rules; use the normative
[core/parser design](../spec/2026-11-06-manifest-core-parsers/design.md) §5 and §8
and [manifest v2 design](../design/manifest-v2.md) for protocol contracts. Distinguish
implemented behavior from planned milestones, and resolve conflicts explicitly.
