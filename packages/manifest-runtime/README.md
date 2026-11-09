# @manifest/runtime

A manifest semantic runtime for Node build scripts, calling `manifest-core`
through WebAssembly. Requires Node.js >= 24.2; browsers and Tauri webviews are not
supported.

Build the untracked wasm artifacts before importing:

```sh
pnpm run build:wasm
```

On import, the package statically imports wasm-bindgen's ESM glue, then
initializes wasm synchronously with `readFileSync` and `initSync`. Function
signatures and handle types come from the generated `wasm/manifest_wasm.d.ts`,
whose parameters and results name the generated types; `pnpm run typecheck`
therefore builds wasm first.

## API and ownership

Exports include `validateManifest`, `compileManifest`, `artifactChecksumKind`,
`selectVariant`, `planVersion`, `planDownload`, `parseVersion`, `finishVersion`,
`finishChecksum`, and `checkPolicy`.
Structured wasm errors become `ManifestRuntimeError`, retaining `code`, `path`,
`message`, and `fields`; other exceptions are rethrown unchanged.

`ValidatedManifest`, `SelectedArtifact`, `VersionPlan`, and `DownloadPlan` are opaque
handles. Release them with `free()` or `Symbol.dispose`. Selection `.value` is the
selected variant. Plan `.value` uses `VersionPlanValue` or `DownloadPlanValue`;
each read returns an independent object with snake_case wire fields.

`src/types.ts` is generated from Rust by `pnpm run generate:schema`; never hand-edit
it. It reuses structural types from `@manifest/metadata`; runtime performs semantic
validation. See [Testing and review](../../docs/development/testing.md) for the
shared native/wasm conformance checks.
