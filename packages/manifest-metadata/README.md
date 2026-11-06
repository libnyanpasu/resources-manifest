# @manifest/metadata

Resource manifest 的元数据包：TS 类型定义、版本常量与 JSON Schema，均由
`crates/manifest-schema-generator` 从 `crates/manifest-metadata` 的 Rust
元类型生成。

**不要手工编辑** `src/types.ts`、`src/version.ts` 或 `schema/`。重新生成：

```sh
pnpm generate:schema
```

## 导出

- `@manifest/metadata`：`types.ts` 全部类型 + `RESOURCE_MANIFEST_VERSION` 常量。
- `@manifest/metadata/types`：仅生成的类型定义。
- `@manifest/metadata/schema/manifest`：编译产物 `ResourceManifest` 的 JSON
  Schema。
- `@manifest/metadata/schema/template`：编译输入 `ResourceManifestTemplate` 的
  JSON Schema。

## 契约层级

TS 类型与 JSON Schema 是**结构契约**，与 wire 格式逐项对齐：

- `manifest_version` 是 `"<major>.<minor>"` 字符串，major 固定为 2；TS 类型为
  `` `2.${number}` ``，Schema 用 `^2\.(0|[1-9][0-9]*)$` 约束格式并拒绝前导零。
  Rust 还要求 minor 可表示为 `u32`。生成的 `RESOURCE_MANIFEST_VERSION` 为 `"2.0"`，
  两个 Schema 的 `$id` 均包含版本路径 `/schema/2.0/`。
- `ResourceChecksum` 只允许 `<algorithm>:<hex>`（TS 模板字面量 / Schema
  `pattern`）。compiled manifest Schema 要求规范化的小写；template Schema
  接受编译器可规范化的任意大小写（与 Rust `ResourceChecksum::new`、参考
  `parseResourceChecksum` 一致）。
- 可选字段在 TS 与 Schema 中只接受"缺席或具体值"，不接受 `null`。

**语义/跨字段规则不属于本包**：tag 组合唯一性、`tag_key` 正确性、同组 tag
互斥、平台默认 variant、registry/mirror 引用、`latest` 必须存在于
`releases`、模板变量可解析等（见 `docs/design/manifest-v2.md` "Validation
invariants"）由编译器/校验器（`@manifest/generator`）在代码层 实现，JSON Schema
不声明这些规则。

## 输入宽松度（有意为之）

Rust 元类型的反序列化把可选字段的显式 `null` 当作 `None`（serde 的 `Option`
语义），比 TS/Schema 契约宽松；序列化永远不会产出 `null`。需要严格 拒绝 `null`
的消费方应对照 JSON Schema 校验（见 `@manifest/downloader` 的 ajv
用法），而不是只依赖 Rust 类型。

Rust 接受同一 major 下的任意合法 minor，`ManifestVersion::is_newer_minor()` 标识
高于当前 `CURRENT_MINOR` 的版本。版本解析错误提供稳定的 `code()` 与
`manifest_version` 路径：格式错误为 `manifest.invalid_version_format`，其他 major
为 `manifest.unsupported_major`，并保留结构字段 `major`。生成的 TS 与 Schema
描述当前 minor 的结构契约；消费者不能仅因旧 Schema 校验失败而拒绝新 minor，
消费侧语义校验以 `validate_manifest` 为准。
