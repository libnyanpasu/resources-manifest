# @manifest/metadata

Resource manifest 的元数据包：TS 类型定义与 JSON Schema，均由
`crates/manifest-schema-generator` 从 `crates/manifest-metadata` 的 Rust
元类型生成。

**不要手工编辑** `src/types.ts` 或 `schema/`。重新生成：

```sh
deno task generate:schema
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

- `manifest_version` 只允许字面量 `2`（TS 字面量类型 / Schema `const`）。
- `ResourceChecksum` 只允许 `<algorithm>:<小写 hex>`（TS 模板字面量 / Schema
  `pattern`；Rust 侧构造与反序列化同样校验并归一化）。
- 可选字段只接受"缺席或具体值"，三处（serde、TS、Schema）都不接受 `null`。

**语义/跨字段规则不属于本包**：tag 组合唯一性、`tag_key` 正确性、同组 tag
互斥、平台默认 variant、registry/mirror 引用、`latest` 必须存在于
`releases`、模板变量可解析等（见 `docs/design/manifest-v2.md` "Validation
invariants"）由编译器/校验器（`@manifest/generator`）在代码层 实现，JSON Schema
不声明这些规则。
