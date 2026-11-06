# Manifest 语义核心、Parser crate 与 wasm 接入 任务

关联设计：`design.md`（同目录）。路径缩写同 design §1。

## 0. 全局约束

1. 遵守 `AGENTS.md`。Rust 代码按 clash-nyanpasu `docs/development/rust.md`（design O-9）：
   - 新错误类型用 snafu 枚举，变体按失败步骤命名，带结构字段和稳定的 `code()`；
   - 需要建立不变量的类型用私有字段；
   - 纯函数和必须处理的结果标 `#[must_use]`；
   - 搬迁时不留 re-export；
   - 不通过重命名 Rust 项来改变 serde wire 名称。
2. 搬迁卡只搬代码。行为变化（design §11.1）各自单独成卡、单独提交，并附带一致性测试集用例；移动中发现的问题记在 PR 描述里，不顺手修。
3. 每个提交都能编译、测试通过，且只做一件事；调用方在同一提交里迁完。
4. `manifest-parser`、`manifest-core` 不加任何 IO 或异步运行时依赖（design G1）。新增依赖写进根 `Cargo.toml` 的 `[workspace.dependencies]`。
5. **发布前版本号固定为 `"2.0"`（design O-11）**：本 spec 内的契约改动都不升 minor，不改 `CURRENT_MINOR`。
6. 每张卡的本地检查：
   - `cargo clippy --workspace --all-targets`
   - `cargo test --workspace`
   - `pnpm check`
   - `pnpm test`
   - `pnpm check:generated`（无差异，或只出现 design G7 列出的预期变化，并写进 PR 描述）
   - Phase C 起，`pnpm test` 前先跑 `pnpm build:wasm`
7. `rg` 的"无残留"检查以退出码 1 为准，退出码 2 视为检查失败。
8. 开工前确认在 worktree 还是当前检出上工作。Phase 0 推荐当前检出；Phase A 起推荐 worktree。

## 1. PR 划分

| PR    | 卡                     | 依赖   | 优先级 | 说明                                      |
| ----- | ---------------------- | ------ | ------ | ----------------------------------------- |
| PR-S0 | S0a、S0b               | —      | P0     | 协议修订和测试集骨架；只改文档和 fixture  |
| PR-S1 | S0c、S0d、S0e          | —      | P0     | 契约加固，可与 PR-S0 并行                 |
| PR-S2 | S0f → S0g              | S1     | P0     | `major.minor` 版本；消费侧宽松解码        |
| PR-A  | A1 → A2 → A3 → A4 → A5 | S0、S2 | P1     | A1 只搬代码；其余各卡是独立的行为提交     |
| PR-B1 | B1                     | A      | P1     | `manifest-core` 与 `validate_manifest`    |
| PR-B2 | B2 → B3 → B4           | B1     | P2     | 模板引擎、计划与 `ChecksumKind`、参考差分 |
| PR-C  | C1 → C2 → C3           | B2     | P2     | wasm、runtime、Node 一致性测试            |
| PR-C4 | C4                     | C      | P2     | 构建期下载校验胶水脚本                    |
| —     | D1 到 D6               | C      | P3     | 里程碑卡，开工前细化                      |

---

## S0a — 修订 `docs/design/manifest-v2.md`

- [ ] 按 design §5.1 改写 §Registry and mirror model、§Backend resolution flow，并写清 `fallback_checksum`、`checksum_remote` 的去向：
  - 新增 §Checksum kinds：四种 kind 的固定小写 snake_case wire 名称、静态推导与执行时 kind 的区别、推荐展示表、宿主同时暴露有效策略决定、默认只接受 `authoritative`、禁止隐式从响应头取 checksum、签名只覆盖 manifest；
  - 写明 UI 在选择或更新内核前应展示 kind（V-1）。
- [ ] 按 design §5.3 改写 §Supported remote version parsers 与 §Supported remote checksum parsers：逐个 parser 写清精确语义，加入 `header` parser，parser 的输入为 body 加 headers（V-10），regex 方言是 Rust `regex`。
- [ ] 新增以下小节：
  - [ ] §Versioning（design §5.4，含 minor 与 major 变更清单、发布前冻结在 2.0）
  - [ ] §Template value classes（§5.2）
  - [ ] §Limits（§5.5）
  - [ ] §Error codes（§5.6）
  - [ ] §Consumers（§5.7）
- [ ] 把 `D/:214-216` 改为"Rust consumers depend on `manifest-core`"（V-8）；`D/:333` 增加"compiler preflight"的边界说明（V-7）；文中所有示例的 `manifest_version` 改为 `"2.0"`（V-9）。
- [ ] §Validation invariants 中每一条后标注由谁检查：`validate_manifest` 或 compile。

**验证**

- `rg -n "should implement the same deterministic semantics" docs/design/manifest-v2.md` 退出码 1。
- `rg -n '"manifest_version": 2\b' docs/design/manifest-v2.md` 退出码 1。
- design §11.1 中的每个 V 编号都能在 `D/` 中找到对应段落（PR 描述里列出对照表）。
- `pnpm fmt:check` 通过。

## S0b — 一致性测试集骨架

- [ ] 新建 `conformance/`，内含：
  - `README.md`：用例格式、来源、"缺陷 ↔ 用例"表；
  - `parsers/version.json`、`parsers/checksum.json`；
  - `validate/manifest.json`、`compat/manifest.json`；
  - `kind/artifact.json`；
  - `resolve/`（B4 填充）。
- [ ] 用例格式：`{ "name", "input": {...}, "expect": { "ok": ... } | { "error": { "code", "path"?, "fields"? } } }`，其中 `name` 全局唯一。parser 用例的 `input` 含 `response: { body, headers }`。
- [ ] 缺陷用例，期望值按 design §5.3 修正后的结果填：
  - [ ] `github-release-assets`：当前资产没有 digest 时，期望 `parser.digest_missing`
  - [ ] `github-release-assets`：`selected.zip` 不得命中 `prefix-selected.zip`
  - [ ] `bsd`：标签为 `SHA512` 而配置为 `sha256` 的行要跳过（期望 `parser.not_found`）
  - [ ] `text`：`trim: false` 且 body 带换行时，期望 `parser.invalid_version`
  - [ ] version 为 `alpha?x=1#f` 时，期望 `parser.invalid_version`
  - [ ] `gnu`：同名两行、digest 不同时，期望 `parser.ambiguous`
  - [ ] `regex`：`group` 超出捕获组数量时，期望 `parser.group_out_of_range`
  - [ ] 没有声明 `header` 规则时，响应里的 `Digest` 头不被使用（`checksum` parser 只看 body）
- [ ] `header` parser 用例：hex 和 base64 各一；头不存在；同名多值冲突；名称大小写不敏感；非法 base64 → `parser.invalid_encoding`；非法 hex 或长度错误 → `parser.invalid_checksum`。
- [ ] `github-release-assets` 用例：href 最后路径段百分号解码后匹配 artifact，显示文本不参与名称判断。
- [ ] `compat` 用例（design G8）：`"2.7"` 被接受且标 `newer_minor`；`"3.0"` 报 `manifest.unsupported_major`；`2` 这个整数和 `"2.07"` 均报 `manifest.invalid_version_format`；未知的 platform key 被跳过；未知 parser type 只让该产物报 `resolve.unsupported_parser`。逐产物投影携带 kind：未知 parser 为 `none`，受支持产物按 §5.1 规则 3 推导。
- [ ] `kind` 用例：static 有 checksum、static 无 checksum、remote 有规则、remote 无规则，分别对应四种结果（design §5.1 第 3 条）。
- [ ] 正常路径用例：至少一个 parser 一个，期望值取参考实现的输出。

**验证**

- README 的"缺陷 ↔ 用例"表覆盖 design §1.2、§1.3 中所有带"复现"标注的行。
- 用一个临时脚本检查所有 `name` 唯一（写进 PR 描述，不入库）。

## S0c — `group` 上限与模板侧 `deny_unknown_fields`

- [ ] `M/lib.rs:316` 的 `group` 加 schemars 上限 `4294967295`，确认生成的 Schema 中出现 `maximum`。
- [ ] 只给模板侧**专用**类型加 `#[serde(deny_unknown_fields)]`：`ResourceManifestTemplate`、`ResourceTemplate`、`ResourceVariantTemplate`、`ResourceTagDefinition`。
  - [ ] 同时用于编译产物的共用类型（`RegistryDefinition`、`MirrorDefinition`、`ResourceArtifactTemplate` 及其子类型）不加，保持宽松（design §5.4 第 5 条），在 PR 描述中列出共用类型清单。
- [ ] 模板 Schema 测试补一条：带未知字段的模板被拒。

**验证**

- `pnpm check:generated` 的差异只包含 `maximum` 和 `additionalProperties: false`，并写进 PR 描述。
- `cargo test -p manifest-metadata` 增加一个测试：模板中带未知字段时反序列化失败；`parses_the_reference_compiled_manifest` 仍然通过。

## S0d — 版本常量改为生成

- [ ] `G/main.rs` 生成 `packages/manifest-metadata/src/version.ts`，导出 `RESOURCE_MANIFEST_VERSION`；值取自 Rust 常量，S0f 之后变为 `"2.0"`。
- [ ] 删除 `packages/manifest-metadata/src/mod.ts:7` 的手写常量，改为 `export * from "./version.ts";`。
- [ ] `generate:schema` 的 prettier 目标加上该文件。

**验证**

- `rg -n "RESOURCE_MANIFEST_VERSION = " packages/manifest-metadata/src/mod.ts` 退出码 1。
- `pnpm check:generated` 无差异；`pnpm test` 通过。

## S0e — 收窄生成器的后处理

- [ ] `G/main.rs:102`：`manifest_version` 的改写只作用于 `ResourceManifest` 和 `ResourceManifestTemplate` 这两个类型，并统计改写次数，必须等于 2。S0f 会把改写目标从字面量 `2` 换成模板字面量类型。
- [ ] `G/main.rs:157`：只在 schema 节点上删除 `format`，且只删整数和浮点格式值（`uint*`、`int*`、`float`、`double`）；遇到其他 `format` 值时报错。
- [ ] 加一项断言：类型图中 `f64` 只出现在 `FiniteF64` 一处。
- [ ] 为上述三点各加一个单元测试。

**验证**

- `cargo test -p manifest-schema-generator` 通过；`pnpm check:generated` 无差异。
- 反向检查：临时给某个 struct 加一个 `f64` 字段，生成器报错。

## S0f — `manifest_version` 改为 `major.minor`（行为卡）

- [ ] `M/lib.rs`：
  - `ManifestVersion { minor: u32 }`（私有字段，提供 `minor()` 访问器）；
  - 常量 `MANIFEST_MAJOR = 2`、`CURRENT_MINOR = 0`；
  - Serialize 输出 `"2.<minor>"`；
  - Deserialize 按 design §5.4 第 2 条处理：不是字符串或格式不对（含 minor 前导零，如 `"2.07"`）时报 `invalid_version_format`，major 不等于 2 时报 `unsupported_major`，minor 任意。
  - `RESOURCE_MANIFEST_VERSION` 改为 `"2.0"` 这个字符串常量。
- [ ] `ManifestVersion::is_newer_minor(&self) -> bool`。
- [ ] JsonSchema：`{ "type": "string", "pattern": "^2\\.(0|[1-9][0-9]*)$" }`。生成器把 TS 类型改写为 `` `2.${number}` ``（替换 S0e 的字面量 `2`，仍按计数断言）。
- [ ] 生成器给两个 Schema 加 `$id`，路径中带 `2.0`（design §5.4 第 5 条），具体 URI 前缀在 PR 中定，写进 design §11.2。
- [ ] 同一提交把以下各处改为 `"2.0"`：快照 `crates/manifest-metadata/tests/fixtures/resources.json`、Rust 测试中的样例、两个 `main_test.ts` 中的样例（迁移不变量第 1 条）。
- [ ] 更新 `packages/manifest-metadata/README.md` 的契约层级说明。

**验证**

- `pnpm check:generated` 的差异只包含 `manifest_version` 的类型与 pattern、`$id`、版本常量。
- `rg -n '"manifest_version": 2\b' crates packages docs` 退出码 1。
- `conformance/compat` 中有关版本格式的用例在 Rust 端通过（运行器在 A2 建好之前，先以 metadata 的单元测试形式覆盖）。

## S0g — 编译产物侧对新 minor 的宽松解码（行为卡）

- [ ] 编译产物的 `artifacts`（`BTreeMap<SupportedPlatform, _>`）在反序列化时跳过未知的 platform key。用 `deserialize_with` 实现，只作用于 `ResourceVariant`，模板侧 `ResourceVariantTemplate` 保持严格。
  - [ ] `ResourceDefinition.default_variants` 同样跳过未知 key。
- [ ] 两个 parser 枚举加保留原始未知 `type` tag 的 `Unsupported { parser_type: String }` 变体，消费侧自定义解码以支持 `resolve.unsupported_parser` 的 `{ parser_type }` 字段（不能用会丢失 tag 的 `#[serde(other)]` 单元变体），并从 TS 与 Schema 导出中隐藏：先确认 `#[schemars(skip)]` 与 specta 的变体 skip 在当前版本上可用；不可用时由生成器做后处理并计数断言，二者择一，在 PR 中说明。
  - [ ] 模板侧遇到 `Unsupported` 时报错：模板必须使用当前 minor 已知的 parser。
- [ ] 加测试：一份 `"2.7"` 的 manifest，带一个未知 platform key 和一个未知 parser type，能反序列化成功；已知部分的数据完整。

**验证**

- `pnpm check:generated` 无差异（`Unsupported` 不出现在生成物里）。
- `rg -n "Unsupported" packages/manifest-metadata/src/types.ts packages/manifest-metadata/schema` 退出码 1。
- 快照 roundtrip 测试仍然通过。

---

## A1 — 新建 `manifest-parser`，搬入 checksum 与 parser 定义

- [ ] 新建 `crates/manifest-parser`，依赖 serde、schemars、specta，加入 workspace。
- [ ] `ChecksumAlgorithm`、`ParseChecksumError`、`ResourceChecksum` 与两个 parser 枚举（含 S0g 的 `Unsupported`）搬入 `manifest-parser/src/{checksum.rs,version.rs,remote_checksum.rs}`；`lib.rs` 只做声明与导出。
- [ ] metadata 依赖 parser，改为 `use manifest_parser::{...}`；不加 `pub use`。
- [ ] checksum 的单元测试随类型搬走。

**验证**

- `pnpm check:generated` 无差异。
- `cargo tree -p manifest-parser -e normal` 中不含 `manifest-metadata`。
- `rg -n "pub use manifest_parser" crates/manifest-metadata` 退出码 1。
- 搬迁前后各导出一次 `cargo test -p manifest-metadata -p manifest-parser -- --list --format terse` 的清单，排序后只有模块前缀变化。

## A2 — `RemoteResponse`、version parser 与 `VersionLabel`（行为卡）

- [ ] `RemoteResponse<'a> { body: &'a str, headers: &'a [(String, String)] }`，提供不区分大小写的 header 查找，以及"多值且值不同时报冲突"的辅助函数。
- [ ] `VersionLabel` newtype：私有字段，实现 `FromStr`、`Display`、`AsRef<str>`，字符集按 design §5.2。
- [ ] `Limits` 结构（design §5.5）实现 `Default`；宿主只能调低。
- [ ] `RemoteVersionParser::compile(&self, &Limits) -> Result<CompiledVersionParser, ParseError>`；`CompiledVersionParser::parse(&self, &RemoteResponse) -> Result<VersionLabel, ParseError>`；`Unsupported` 在 `compile` 时报 `resolve.unsupported_parser`。
- [ ] `ParseError` 用 snafu，实现 `code()`；变体、稳定结构字段和 `path` 语法按 design §5.6；运行器比较 `code`、声明的 `fields`，并精确比较声明的 `path`。
- [ ] 新增依赖 `regex`，用 `RegexBuilder::size_limit` 限制编译大小。
- [ ] 两个 parser 枚举加 `#[non_exhaustive]`（design §6.6）。
- [ ] 测试集运行器 `crates/manifest-parser/tests/conformance.rs` 读取 `conformance/parsers/version.json`，以 `name` 作子测试名执行。
- [ ] 用快照中的全部版本做回归，确认它们都满足 `VersionLabel`。
- [ ] 增加 design §5.5 上限的一致性用例：`body_too_large`、regex pattern 长度与编译大小上限。

**验证**

- S0b 中 version 类的缺陷用例全部通过。
- 反向检查：临时把 `trim: false` 改回无条件 trim，对应用例失败。

## A3 — `checksum`、`gnu`、`bsd`（行为卡）

- [ ] `RemoteChecksumParser::parse(&self, &RemoteResponse, artifact: &str, &Limits) -> Result<ResourceChecksum, ParseError>`。
- [ ] `github-release-assets` 分支暂时返回 `ParseError::Unsupported { parser }`，不移植参考实现的邻近正则；A4 删除这个变体。
- [ ] 手写行解析器，处理 CRLF，不做正则转义。
- [ ] `checksum`、`gnu`、`bsd` 只读 body，不看 headers。
- [ ] 测试集运行器加载 `conformance/parsers/checksum.json`。

**验证**

- S0b 中 `gnu`、`bsd` 的缺陷用例和"未声明 header 不被使用"的用例通过；正常路径用例与参考实现的结果相同。

## A4 — `github-release-assets` 结构化解析（行为卡）

- [ ] 采集真实页面 fixture，放在 `conformance/fixtures/github-expanded-assets/`，覆盖 design §6.4 第 1 条的四种情况；README 记下 URL 与采集时间。
- [ ] 候选 tokenizer 各做一个最小原型，记录对 fixture 是否正确，以及 `wasm32-unknown-unknown` release 下的 `.wasm` 体积增量；结论写进 design §11.2 O-3。
- [ ] 实现条目切分与精确名称匹配（href 最后路径段百分号解码，不使用显示文本），按 §5.3 报错；删除 A3 留下的 `Unsupported` 变体。

**验证**

- S0b 中两个 `github-release-assets` 缺陷用例通过；`rg -n "2048" crates/manifest-parser` 退出码 1。
- 每个 fixture 中所有带 digest 的资产都能按名称取回正确的 digest。

## A5 — `header` checksum parser（行为卡）

- [ ] `RemoteChecksumParser::Header { name: String, algorithm: ChecksumAlgorithm, encoding: HeaderEncoding }`；`HeaderEncoding` 有 `Hex`、`Base64` 两个变体，按 kebab-case 序列化。
- [ ] 解码规则按 design §5.3；新增依赖 `base64`。非法 base64 → `parser.invalid_encoding`；非 hex、奇数个 hex 字符或 digest 长度错误 → `parser.invalid_checksum`；单个 header 值超限 → `parser.body_too_large`，带 `{ limit, actual }`（UTF-8 字节数）；补充超限用例。
- [ ] 模板侧校验：`name` 非空，且是合法的 header 名称（token 字符）。
- [ ] 重新生成契约。

**验证**

- S0b 中 `header` 类的用例通过。
- `pnpm check:generated` 只新增 `header` 变体与 `HeaderEncoding`（design G7）。

---

## B1 — 新建 `manifest-core` 与 `validate_manifest`

- [ ] 新建 `crates/manifest-core`，依赖 parser、metadata、snafu。
- [ ] `ValidatedManifest`：私有字段，只读访问，暴露 `newer_minor()`；`validate_manifest(raw) -> Result<ValidatedManifest, ValidationErrors>` 一次收集全部违规。
- [ ] 每条不变量一个 `validate.*` 码（design §7.1 列表），在 PR 中补全 design §5.6 的 `validate.` 行。
- [ ] 已知 parser 在校验时调用 `compile`；`Unsupported` 不在这里报错（design §7.1）。
- [ ] 测试集运行器加载 `conformance/validate/` 与 `conformance/compat/`；每条不变量一正一反（design G6）。
- [ ] 快照必须校验通过。

**验证**

- `cargo test -p manifest-core` 通过；用例数 ≥ 2 × 不变量条数；`compat` 用例通过（design G8）。
- `cargo tree -p manifest-core -e normal` 中没有 HTTP 或异步运行时。

## B2 — 模板引擎（行为卡）

- [ ] `TemplateValue::{Authored, Literal}`；按 design §7.2 的优先级合并作用域。
- [ ] 渲染：未知变量、环、深度、输出长度、Literal 的字符集，分别报 `template.*`。
- [ ] 最终 URL：`url` crate 解析；协议只允许 http(s)；带 userinfo 或 fragment 时报 `url.forbidden_component`。
- [ ] 测试集用例：`alpha?x=1#f` 作为宿主传入的 version 时被拒；`release: "{version}"` 能正常展开。

**验证**

- 新用例通过；对快照中所有 static 历史版本，渲染出的产物 URL 中不含 `?` 和 `#`。

## B3 — 变体选择、计划、`ChecksumKind` 与校验策略（行为卡）

- [ ] `ChecksumKind`（Rust 变体 `Authoritative`、`Parsed`、`Snapshot`、`None`），derive `specta::Type`，加 `#[non_exhaustive]` 与 `#[serde(rename_all = "snake_case")]`；wire 名称固定为 `authoritative`、`parsed`、`snapshot`、`none`。
- [ ] `artifact_checksum_kind`，以及按 resource 与平台列出各 variant kind 的函数（design §7.3）。
- [ ] `select_variant`、`plan_version`、`plan_download`、`finish_checksum`；计划中含 `canonical_url`、可选的 `mirror_url` 与 `kind`。
- [ ] `VerifyPolicy`（实现 `Default`，默认全为 `false`）、`check_policy`、`VerifyError`（`kind_rejected`、`digest_mismatch`）；结构字段按 design §5.6。
- [ ] 宿主在 kind 旁暴露有效策略决定（用宿主 `VerifyPolicy` 调用 `check_policy`，接受或拒绝及原因）；静态 kind 用于选择前展示，计划与结果携带执行时适用的 kind。推荐展示按 design §5.1，具体 UI 文案不在范围内。
- [ ] 未知 version / checksum parser 的产物静态推导为 `none`，任何校验策略都不能接受；compat 运行器同时断言逐产物支持性和 kind。
- [ ] mirror 的 `registries` 限制与参考实现一致（`R/resource-template.ts:1037-1045`）。
- [ ] 测试集运行器加载 `conformance/kind/`。

**验证**

- `kind` 用例与选择类用例通过。
- 默认策略下 `parsed`、`snapshot`、`none` 都被拒；`allow_parsed` 只放开 `parsed`；任何策略都不接受 `none`。

## B4 — 参考差分

- [ ] 按 design §7.4 生成 `conformance/resolve/reference-golden.json`；生成命令和参考实现的来源（上游 commit）写进 README。
- [ ] 差分测试：对每个条目断言 `canonical_url`、`download_url`、产物名逐字节相同。
- [ ] 不一致项要么修 core，要么在 design §11.1 新增 V 编号并更新 golden。

**验证**

- design G5 成立；PR 描述列出覆盖的 resource × platform 数量。

---

## C1 — `manifest-wasm` 与构建

- [ ] 新建 `crates/manifest-wasm`（`cdylib`），依赖 core、`wasm-bindgen`、`serde-wasm-bindgen`；`wasm-bindgen` 版本与 CLI 一致并锁定（当前本机 0.2.114，以 PR 时为准）。
- [ ] 导出 design §8.1 中的函数；`ValidatedManifest` 作为不透明句柄导出；错误转成 `{ code, path, message, fields }`。
- [ ] 根 `package.json` 增加 `build:wasm`：先检查 CLI 与 crate 版本一致，再执行 cargo 构建和 `wasm-bindgen --target web --out-dir packages/manifest-runtime/wasm`。
- [ ] `.gitignore`、`.prettierignore` 和 oxlint 的忽略列表加上 `packages/manifest-runtime/wasm/`。
- [ ] 在 PR 描述中记录 `.wasm` 原始大小与 gzip 大小、Node 下 `initSync` 的耗时（5 次取中位数），据此定 O-8。

**验证**

- `pnpm build:wasm` 在干净检出上成功；版本不一致时脚本失败并给出提示。
- `cargo clippy -p manifest-wasm --target wasm32-unknown-unknown` 通过。

## C2 — `@manifest/runtime`

- [ ] 新建 `packages/manifest-runtime`（`private: true`），`src/mod.ts` 用 `readFileSync` 加 `initSync` 同步初始化，导出类型化函数。
- [ ] 生成器导出 core 的计划、kind、策略与错误类型，写进 `packages/manifest-runtime/src/types.ts`（生成物，纳入 `generate:schema` 与 `check:generated`）。
- [ ] 错误类 `ManifestRuntimeError extends Error`，带 `code`、`path`、`fields`。
- [ ] `tsconfig.json` 的 `include` 覆盖新包。

**验证**

- `pnpm typecheck` 通过；`pnpm check:generated` 只新增 `types.ts`。
- `node -e` 导入 runtime 并对快照调用 `validateManifest` 与 `artifactChecksumKind` 成功（PR 描述中附上命令）。

## C3 — Node 端一致性测试

- [ ] `packages/manifest-runtime/conformance_test.ts`：遍历 `conformance/**.json`，每条用例一个 `test(name)`，经 runtime 执行。
- [ ] 根 `test` 脚本的 glob 覆盖新包；`pnpm test` 在 `build:wasm` 之后运行，`AGENTS.md` 同步更新命令。
- [ ] 新增 `pnpm check:conformance`：比较 Rust 与 Node 两端的用例名清单。

**验证**

- design G3 成立。
- 反向检查：在 Rust 端临时跳过一条用例，`check:conformance` 失败。

## C4 — 构建期下载校验胶水脚本（行为卡）

- [ ] `packages/manifest-downloader/main.ts` 实现 design §8.4 的命令行与流程，参数解析用 `node:util` 的 `parseArgs`。
- [ ] 开始下载前打印 resource、variant、版本、kind 和宿主 `VerifyPolicy` 下的 `check_policy` 决定（接受或拒绝及原因）；`parsed` 或 `snapshot` 在开关放开后被接受时，输出 `WARN` 行。
- [ ] 下载先写临时文件，`node:crypto` 校验通过后再原子 rename；失败时删除临时文件。
- [ ] HTTP：重定向不超过 5 次；每个请求有超时；body 大小上限与 design §5.5 一致；只把 parser 规则声明的 header 交给 `finishChecksum`。
- [ ] 测试（`node --test`，用本地 `node:http` 服务模拟上游，不访问外网）：
  - [ ] static 产物下载并校验成功
  - [ ] remote 产物在默认策略下报 `verify.kind_rejected`
  - [ ] 同一个 remote 产物在 `--allow-parsed` 下成功
  - [ ] digest 不匹配时报 `verify.digest_mismatch`，且 `--out` 中没有残留
  - [ ] 响应带 `Digest` 头、但 manifest 没有声明 header 规则时，不使用该头

**验证**

- `pnpm test` 中上述用例通过；`rg -n "fetch\(" packages/manifest-downloader` 只出现在 HTTP 模块中。

---

## 里程碑卡（开工前细化为同粒度卡）

### D1 到 D6 — 编译器、快照与签名

- 见 design §9 表格。
- D2 的确定性判据：同一组输入两次编译，输出逐字节相同；写出 `"2.0"`。
- D4 开工前先采集 `latest/version.txt` 的实际响应作为 fixture（O-4）。
- D5 签名另出 spec，范围只覆盖 manifest 本身（O-2）。
