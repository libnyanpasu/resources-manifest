# resources-manifest

资源 manifest（v2）工作区：Rust 定义元类型，Deno 包负责生成与消费。

## 布局约束

- `crates/manifest-metadata`：资源元类型定义，以 serde、schemars、specta 标注。
  强约束内建在类型里：`ManifestVersion`（只接受字面量 2）、`ResourceChecksum`
  （`<algorithm>:<hex>` 校验并归一化）、`FiniteF64`（拒绝 NaN/无穷）。
- `crates/manifest-schema-generator`：从元类型生成 TypeScript 类型定义与 JSON
  Schema，写入 `packages/manifest-metadata`。
- `packages/manifest-metadata`：纯 TypeScript 库，存放生成的类型与 schema
  （`src/types.ts`、`schema/` 均为生成物，禁止手改）。契约层级说明见其 README。
- `packages/manifest-generator`：生成完整 manifest 的 Deno 可执行包。
- `packages/manifest-downloader`：解析 manifest 并按需下载的 Deno 可执行包。
- `files/`：上游参考实现与设计文档（只读参考，不参与 lint/fmt/test）。
- `docs/design/`：设计文档。

## 常用命令

- `deno task generate:schema`：重新生成 TS 类型与 JSON Schema（cargo 运行后 自动
  `deno fmt` 产物）。
- `deno task check`：`deno fmt --check` + `deno lint` + `deno check`。
- `deno task test`：Deno 测试。
- `cargo test --workspace`、`cargo clippy --workspace --all-targets`：Rust 测试
  与静态检查。

## 约定

- 根 `deno.json` 声明 Deno workspace（`packages/*`），跨包按包名引用 （如
  `@manifest/metadata`）。
- 根 `Cargo.toml` 声明 Cargo workspace（`crates/*`），依赖版本集中在
  `[workspace.dependencies]`。
- 修改元类型后必须运行 `deno task generate:schema` 并提交生成物。
