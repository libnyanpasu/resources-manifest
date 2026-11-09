# Repository scripts

TypeScript automation runs directly on Node.js >= 24.2 using only Node built-ins.
Run tasks from the repository root. The root `package.json` is the task catalog;
there is no separate scripts workspace package.

| Task                         | What it does                                                                                                               |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `pnpm run build:wasm`        | Checks the wasm-bindgen CLI version against Cargo.lock, builds the locked release wasm target, and generates runtime glue. |
| `pnpm run build:viewer`      | Builds wasm, then the manifest viewer into `packages/manifest-viewer/dist/`.                                               |
| `pnpm run dev:viewer`        | Builds wasm, then serves the manifest viewer with hot reload at `http://localhost:4321/resources-manifest/`.               |
| `pnpm run generate:schema`   | Runs the Rust schema generator and formats metadata types, version constants, schemas, and runtime types.                  |
| `pnpm run check:generated`   | Regenerates outputs and rejects tracked changes or untracked files in the generated targets.                               |
| `pnpm run check:conformance` | Builds wasm, runs the four Rust corpus runners and the Node runner, and checks executed case-name parity.                  |
| `pnpm run check`             | Runs formatting, lint, and TypeScript checks.                                                                              |
| `pnpm run fmt`               | Formats the repository with Prettier, respecting `.prettierignore`.                                                        |
| `pnpm run fmt:check`         | Checks formatting with Prettier.                                                                                           |
| `pnpm run lint`              | Runs Oxlint.                                                                                                               |
| `pnpm run typecheck`         | Builds wasm, runs the shared TypeScript configuration with no emit, then the viewer's `svelte-check`.                      |
| `pnpm run test`              | Builds wasm and runs `packages/*/*_test.ts` with Node's test runner, including runtime conformance.                        |

Source lives under `src/wasm/`, `src/conformance/`, and `src/generate/`.
`src/shared/repo-paths.ts` owns source-relative repository path resolution.
See [Repository scripts](../docs/development/scripts.md) for organization and
verification rules and [Testing and review](../docs/development/testing.md) for
Rust checks and toolchain prerequisites.
