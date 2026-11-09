# TypeScript code style

This guide applies to handwritten Node.js packages and repository scripts. It
adapts the reference repository's module, naming, reuse, and review conventions to
this workspace. The only UI is `packages/manifest-viewer`, an Astro page with one
Svelte island; its components render values computed by its `src/model.ts`, which
follows this guide.

## Runtime and workspace

- Require Node.js >= 24.2 for native type stripping and `import.meta.main`.
  The root `package.json` pins pnpm through `packageManager`.
- Write only erasable TypeScript: no enums, namespaces, or constructor parameter
  properties. Root `tsconfig.json` enforces `erasableSyntaxOnly`, strict checking,
  `verbatimModuleSyntax`, and `noEmit` with `module: nodenext`.
- Use `.ts` extensions for relative imports and `node:` for Node built-ins.
  Use `import type` for type-only imports.
- `pnpm-workspace.yaml` declares `packages/*`. Import other packages by their public
  name, such as `@manifest/metadata`, with `workspace:*` dependencies. Do not import
  another package's private `src/` path.
- Packages export TS source without a build step. Node does not strip types from
  `.ts` files inside `node_modules`, so this setup currently supports workspace
  consumption. Publishing requires a build step first.

## Formatting, names, and module grouping

Prettier currently uses its defaults: two spaces, double quotes, semicolons, and
trailing commas. `.prettierrc.json` only adds the Svelte and Astro plugins.
Preserve LF. This repository has no import-sorting or Tailwind plugin
configuration; do not copy the reference's quote or semicolon settings. Oxlint
uses `.oxlintrc.json`; root TypeScript checking includes every package except the
viewer, which `pnpm run typecheck` checks with `svelte-check`, and
`scripts/src/**/*.ts`.

Keep imports at the top, grouped as Node built-ins, external/workspace packages,
then local modules. Avoid duplicates and retain domain context in names. Use
PascalCase for types/classes, camelCase for functions/locals, and descriptive
uppercase names for shared fixed limits. Preserve snake_case wire fields.

Group related statements and separate distinct responsibilities with one blank
line: input validation, plan construction, request execution, verification, and
output. Separate a final return from preceding setup, while keeping short early
returns compact. Prettier cannot decide these semantic groups.

Prefer `const` unless reassignment is necessary, strict comparisons, explicit domain
types, and `unknown` plus narrowing for untrusted values. Use `as const` for literal
types where useful. Avoid new `any`, broad assertions, and file-wide checking bypasses.
Use `import.meta.main` to guard package CLI entrypoints that are also imported.

## Reuse constants and runtime checks

Search the owning module and its callers before adding a constant or environment
check. Reuse definitions with the same meaning; keep one-use details local. Promote
shared values only to their smallest actual common owner. Similar numeric literals
with different meanings do not imply a common constant.

Keep constants immutable and safe to import. Runtime-varying values need functions
or explicit inputs rather than stale module-level snapshots. Do not turn a constant
module into hidden mutable state or eager IO. The runtime's Node entry deliberately
initializes wasm on import, and its browser entry only through `initRuntime()`;
preserve that documented boundary rather than spreading it to unrelated modules.

## Errors and host boundaries

- Core owns selection, templates, parsing semantics, plans, and verification policy.
  Use `@manifest/runtime` rather than a second TypeScript implementation.
- Preserve `ManifestRuntimeError.code`, `.path`, `.fields`, and `.message`. Match
  codes and structured fields programmatically; message text is display-only.
  Do not catch unrelated exceptions and silently reinterpret them as core errors.
- Keep HTTP in downloader `http.ts`, with explicit request limits and timeout
  handling. Pass only headers requested by the declared parser. Do not infer a
  checksum from undeclared response headers.
- Stream artifacts into temporary output files, calculate digests during transfer,
  verify before atomic rename, and clean failed temporary files. Do not buffer
  artifacts in memory or replace existing output before verification succeeds.
- Keep diagnostics useful without exposing URL credentials or query secrets.
  CLI failures must retain a nonzero exit status.

## Generated files

Never hand-edit metadata `src/types.ts`, `src/version.ts`, `schema/`, or runtime
`src/types.ts`. Their source is Rust; regenerate with `pnpm run generate:schema`
and verify with `pnpm run check:generated`. Wasm glue under
`packages/manifest-runtime/wasm/` is generated, ignored, and rebuilt with
`pnpm run build:wasm`. Structural types/schemas do not replace runtime semantic
validation or the compiled-manifest compatibility policy.

## What is enforced

| Requirement                                                          | Enforcement                       |
| -------------------------------------------------------------------- | --------------------------------- |
| Mechanical formatting                                                | Prettier (`pnpm run fmt:check`)   |
| Configured correctness and lint rules                                | Oxlint (`pnpm run lint`)          |
| Strict types, erasable syntax, and imports                           | TypeScript (`pnpm run typecheck`) |
| Generated output drift, including untracked files                    | `pnpm run check:generated`        |
| Shared semantics and executed case-name parity                       | `pnpm run check:conformance`      |
| Meaningful names, constants, module grouping, and boundary ownership | Mandatory code review             |

Run `pnpm run check` and the relevant tests in [Testing and review](testing.md).
Keep suppressions narrow and justified; passing tools does not replace semantic
review.
