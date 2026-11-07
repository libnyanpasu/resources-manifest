# @manifest/runtime

Node 构建脚本使用的 manifest 语义运行时，通过 WebAssembly 调用 `manifest-core`。
仅支持 Node.js >= 24.2，不支持浏览器或 Tauri webview。

导入前先构建未入库的 wasm 产物：

```sh
pnpm build:wasm
```

包在导入时用 Node 的 `createRequire` 同步加载 wasm-bindgen 的 ESM glue，
再用 `readFileSync` 和 `initSync` 同步初始化 wasm。glue 的接口在源码中声明，
类型检查不依赖构建目录，因此未运行 `build:wasm` 的 checkout 也能运行 `pnpm check`。

导出 `validateManifest`、`artifactChecksumKind`、`selectVariant`、`planVersion`、
`planDownload`、`parseVersion`、`finishChecksum` 和 `checkPolicy`。
wasm 的结构化错误转换为 `ManifestRuntimeError`，保留 `code`、`path`、`message`
和 `fields`；其他异常原样抛出。

`ValidatedManifest`、`SelectedArtifact`、`VersionPlan`、`DownloadPlan` 是不透明句柄，
用 `free()` 或 `Symbol.dispose` 释放。selection 的 `.value` 是所选 variant；
两个计划的 `.value` 分别使用 `VersionPlanValue` 和 `DownloadPlanValue` 类型，
每次读取返回独立对象，wire 字段保持 snake_case。

`src/types.ts` 由 `pnpm generate:schema` 从 Rust 生成，不要手工编辑。
它复用 `@manifest/metadata` 的结构类型；语义校验通过 runtime 完成。
