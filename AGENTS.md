# resources-manifest

资源 manifest（v2）工作区：Rust 定义元类型，Node.js（pnpm workspace）包负责生成与消费。

## 布局约束

- `crates/manifest-metadata`：资源元类型定义，以 serde、schemars、specta 标注。
  强约束内建在类型里：`ManifestVersion`（接受 major 为 2、minor 为无前导零的 u32 的版本字符串，如 `"2.0"`）、`ResourceChecksum`
  （`<algorithm>:<hex>` 校验并归一化）、`FiniteF64`（拒绝 NaN/无穷）。
- `crates/manifest-parser`：checksum / parser 的 wire 类型与无 IO 实现，以及
  `Limits`、`RemoteResponse`、`ParseError`；不引入 IO / async 依赖。
  metadata 依赖 parser，禁止反向依赖；`ResourceChecksum` 的实现也位于此 crate。
- `crates/manifest-core`：编译产物的纯语义校验与 `ValidatedManifest`、模板引擎、variant
  选择、版本与下载计划、`ChecksumKind` 和 `VerifyPolicy`；依赖 metadata 和 parser，
  不引入 IO / async 依赖，metadata 和 parser 禁止反向依赖 core。
- `crates/manifest-schema-generator`：从元类型生成 TypeScript 类型定义、版本常量与 JSON
  Schema，写入 `packages/manifest-metadata`。
- `packages/manifest-metadata`：纯 TypeScript 库，存放生成的类型与 schema
  （`src/types.ts`、`src/version.ts`、`schema/` 均为生成物，禁止手改）。契约层级说明见其 README。
- `packages/manifest-generator`：生成完整 manifest 的 Node.js 可执行包。
- `packages/manifest-downloader`：解析 manifest 并按需下载的 Node.js 可执行包。
- `conformance/`：语言无关的一致性语料，Rust 运行器位于
  `crates/manifest-parser/tests/conformance.rs`（parser 与全局名称检查）和
  `crates/manifest-core/tests/conformance.rs`（validate / compat）。
- `files/`：上游参考实现与设计文档（只读参考，不参与 lint/fmt/test）。
- `docs/design/`：设计文档。

## 运行时

- Node.js >= 24.2（直接运行 `.ts`：依赖原生 type stripping 与 `import.meta.main`），
  pnpm 版本由根 `package.json` 的 `packageManager` 锁定。
- TS 源码只能使用可擦除语法（`erasableSyntaxOnly`：禁止 `enum`、`namespace`、
  参数属性等），相对导入写 `.ts` 扩展名。
- 包以 TS 源码形式导出（不构建）；Node 不会对 `node_modules` 内的 `.ts`
  做 type stripping，因此目前只适合 workspace 内消费，发布前需增加构建步骤。

## 常用命令

- `pnpm install`：安装依赖。
- `pnpm generate:schema`：重新生成 TS 类型、版本常量与 JSON Schema（cargo 运行后自动
  `prettier --write` 产物）。
- `pnpm check:generated`：重新生成后检查目标目录的 Git 状态（包含未跟踪文件），
  用于 CI 防漂移。
- `pnpm check`：`prettier --check` + `oxlint` + `tsc --noEmit`。
- `pnpm fmt`：`prettier --write .`。
- `pnpm test`：`node --test` 运行 `packages/*/*_test.ts`。
- `cargo test --workspace`、`cargo clippy --workspace --all-targets`：Rust 测试
  与静态检查。

## 约定

- 根 `pnpm-workspace.yaml` 声明 pnpm workspace（`packages/*`），跨包按包名引用
  （如 `@manifest/metadata`，依赖写 `workspace:*`）。
- 根 `tsconfig.json` 统一类型检查所有包（`module: nodenext`）。
- 根 `Cargo.toml` 声明 Cargo workspace（`crates/*`），依赖版本集中在
  `[workspace.dependencies]`。
- 修改元类型后必须运行 `pnpm generate:schema` 并提交生成物；
  `pnpm check:generated` 必须无差异。
- 真实编译产物 manifest 的测试快照在
  `crates/manifest-metadata/tests/fixtures/resources.json`（受跟踪，因为
  `files/` 参考目录不入库；`.prettierignore` 排除以保持原样）；Rust 与 Node
  测试共用这份快照。
