# Manifest 语义核心、Parser crate 与 wasm 接入 设计

**日期：** 2026-11-06
**基线：** `chore/migrate-to-pnpm @ 8bb7752`（`main @ f1f82a7` 加 pnpm 迁移）
**来源：** 2026-11-06 的仓库架构审查（codex 后端视角、kimi 消费端视角、Claude 逐条对源码复核）；维护者提议"把 parser 的算子定义与实现抽成独立 crate，前端经 wasm 接入复用"；维护者 2026-11-06 的决策（O-1、O-6、O-7、O-11，§11.2）。
**范围：** 审查发现的修复与新方向的优先级（§2），以及 Phase 0（协议冻结）、Phase A（`manifest-parser`）、Phase B（`manifest-core`）、Phase C（wasm 与构建期胶水脚本）。Phase 0–C 写到可直接执行的卡片粒度；Phase D（编译器与快照）写到里程碑粒度，开工前再细化。
**不在范围：** 见 §12。
**权威顺序：** `AGENTS.md` > 本设计 > `tasks.md` > `docs/design/manifest-v2.md`（本 spec 的 S0a 卡会按 §5 修订它）。Rust 代码规则采用 clash-nyanpasu `docs/development/rust.md`（O-9），下文以 `rust.md §x` 引用。

---

## 0. 结论摘要

1. **先定协议，再搬代码（Phase 0）。** 参考实现的几处行为是错的或未定义的：`github-release-assets` 会取到相邻资产的 digest，`trim: false` 不起作用，BSD 解析不核对算法标签，`regex` 的方言未定义。直接移植等于把缺陷固化进第二种语言。Phase 0 先修订协议文本，并建立一份 Rust 和 Node 共用的一致性测试集，把已复现的缺陷写成用例。
2. **采纳拆 crate 的方向，但分两层。** `manifest-parser` 是叶子 crate，拥有 checksum 与 parser 的定义和实现，不依赖 metadata。`manifest-core` 在它之上，拥有语义校验、模板渲染、variant 选择和下载计划。只抽 parser 解决不了偏差问题：parser 只有约 80 行，偏差风险主要在变量合并、模板渲染和语义校验。
3. **两层都不做 IO（sans-IO）。** 网络请求、凭据、重定向、超时、响应大小上限都由宿主负责。core 输出请求计划，宿主把响应（body 与 headers）交回 parser。
4. **wasm 只给 Node 包用（O-6，已定）。** 用途是构建期的胶水脚本下载并校验资源。Rust 宿主（clash-nyanpasu 后端）直接链接 crate；webview 只展示后端给出的结果，不加载 wasm。
5. **默认只接受权威 checksum（O-1，已定）。** 只有 manifest 内嵌、随 manifest 签名的 checksum 才算权威。远端经 parser 规则取得的 checksum（不论来自 body 还是 header，不论走 canonical 还是 mirror）都标为 `parsed`，默认被拒。从 header 取 checksum 必须由 parser 规则显式声明，宿主不得自行从响应头推断。每个产物的 `ChecksumKind` 不联网就能从 manifest 推出，前端据此在更新 alpha 这类内核前提示用户（§5.1）。
6. **版本采用 `major.minor`（O-7，已定，参考 Docker）。** `manifest_version` 改为字符串 `"2.0"`。兼容更新只升 minor；消费者接受同一 major 下的任意 minor。**包发布前 schema 版本保持 `2.0`**，发布前的契约改动都并入 2.0，不升版本（O-11）。
7. **类型导出只有 specta 一个来源。** wasm 边界用 `serde-wasm-bindgen` 传值，TS 类型复用生成物，不引入 tsify（V-6）。
8. **优先级：**
   - P0：协议冻结、一致性测试集、契约加固、`major.minor` 版本与对新 minor 的宽松解码；
   - P1：parser crate（含 `header` parser）与 `validate_manifest`；
   - P2：resolver（含模板注入修复、`ChecksumKind`）与 wasm 胶水脚本；
   - P3：编译器与快照（§2）。

---

## 1. 现状事实（已核对源码，`8bb7752`）

路径缩写：`M/` = `crates/manifest-metadata/src/`，`G/` = `crates/manifest-schema-generator/src/`，`R/` = `files/scripts/`（上游参考实现，未入库），`D/` = `docs/design/manifest-v2.md`。

### 1.1 契约与实现的归属

| 事实                                                                                                             | 位置                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| 本仓库只有结构契约：Rust 元类型，以及由它生成的 TS 类型和 JSON Schema                                            | `M/lib.rs`；`packages/manifest-metadata/`                                                                                   |
| 编译器、语义校验、resolver、parser 只存在于未入库的参考 TS 中                                                    | `R/resource-template.ts:808`（compile）、`:1166`（resolve version）、`:1211`/`:1233`（parser）、`:1288`（resolve download） |
| 设计文档要求 Rust 消费者"实现相同的语义"，等于规定了两套实现                                                     | `D/:214-216`                                                                                                                |
| `generator`、`downloader` 两个包只是打印版本号的占位实现                                                         | `packages/manifest-generator/main.ts:3`；`packages/manifest-downloader/main.ts:3`                                           |
| Rust 反序列化和 JSON Schema 都不检查跨字段规则；重复 variant、伪造 `tag_key`、悬空的 `default_variants` 都能通过 | `M/lib.rs:12-14`；codex 复现                                                                                                |
| `RESOURCE_MANIFEST_VERSION` 在 TS 侧手写，没有从 Rust 生成                                                       | `packages/manifest-metadata/src/mod.ts:7`                                                                                   |

### 1.2 信任与 URL 构造

| 事实                                                                                              | 位置                                                                     |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| 同一个 mirror 用于二进制、远端 version 和远端 checksum 元数据                                     | `D/:22-24`、`:282-286`；`R/resource-template.ts:1118-1128`、`:1368-1373` |
| `checksum` 可选；缺失时下载流程如何处理没有规定；允许 md5                                         | `M/lib.rs:268-280`、`:122-129`；`D/:229-232`                             |
| 结果区分了 `checksum`、`fallback_checksum`、`checksum_remote`，但没有统一的"来源种类"可供 UI 判断 | `R/resource-template.ts:1390-1396`；`D/:191-199`                         |
| manifest 本身没有签名，也没有声明信任锚点                                                         | `D/` 全文                                                                |
| 模板变量会递归展开：变量值里的 `{x}` 会继续被替换                                                 | `R/resource-template.ts:945-968`                                         |
| 远端 version 只检查非空，不限制字符集；URL 只检查协议是 http(s)                                   | `R/resource-template.ts:1222-1225`、`:970-981`                           |
| 远端 version 为 `alpha?x=1#f` 时，生成的产物 URL 的 query 和 fragment 会被改写                    | codex 复现                                                               |

### 1.3 parser 语义

| 事实                                                                                                                                                              | 位置                                                                         |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| parser 的输入只有 body，没有 headers                                                                                                                              | `R/resource-template.ts:1211-1214`、`:1233-1237`                             |
| `text` parser 设置 `trim: false` 无效：返回前无条件 `.trim()`                                                                                                     | `R/resource-template.ts:1217`、`:1225`                                       |
| `regex` 的语义就是 JS `RegExp`（无 flag），校验阶段只做 `new RegExp`                                                                                              | `R/resource-template.ts:412-429`、`:1219`                                    |
| `core-resource-templates.ts` 只使用 `text` parser，没有存量 regex pattern                                                                                         | `R/core-resource-templates.ts` 全文（`rg 'regex\|pattern'` 无匹配）          |
| `bsd` 用 `[A-Za-z0-9-]+` 匹配算法标签，再按配置的算法解释 digest，不核对标签                                                                                      | `R/resource-template.ts:1254-1262`                                           |
| `github-release-assets` 在产物名前后 2048 字符内找第一个 digest：当前资产没有 digest 时会取到相邻资产的；按子串匹配时 `selected.zip` 会命中 `prefix-selected.zip` | `R/resource-template.ts:1265-1279`；codex 复现两种情况                       |
| 生成端读取结构化的 GitHub API `asset.digest`；消费端却抓 HTML，两端的 digest 来源不一致                                                                           | `R/manifest.ts:104-125`；`R/core-resource-templates.ts:55`                   |
| Rust 的 `group` 是 `u32`，Schema 只写了非负整数，没有上限                                                                                                         | `M/lib.rs:316`；`packages/manifest-metadata/schema/manifest.schema.json:378` |

### 1.4 编译与生成

| 事实                                                                                                                                                                                                   | 位置                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 编译阶段只检查注册表模板是否非空、变量名是否合法，从不试渲染；`https://{missing}/{artifact}` 能通过编译                                                                                                | `R/resource-template.ts:579-588`、`:808-863`；`D/:333` 声称会拒绝                                                                         |
| 排序用 `localeCompare`，结果依赖运行时的 locale                                                                                                                                                        | `R/resource-template.ts:558`、`:800`、`:838`                                                                                              |
| 时间戳在生成时取 `new Date()`，网络抓取和编译没有分开                                                                                                                                                  | `R/generate-latest-version.ts:126-128`                                                                                                    |
| Clash Rust alpha：生成端用 `git/ref/tags/latest` 的 commit 加稳定版版本号拼出版本，消费端却去请求 `latest/version.txt`，两种算法不等价                                                                 | `R/manifest.ts:327-345`；`R/core-resource-templates.ts:131-135`、`:380-386`；`crates/manifest-metadata/tests/fixtures/resources.json:321` |
| 2026-11-08 复核：上游已迁到 `ibigbug/clash-rs`；`Watfaq/clash-rs` 跳转到已落后的 `Watfaq-legacy/clash-rs`。两处 `latest/version.txt` 都返回 404；clash-nyanpasu 后端用两到三次 GitHub API 请求拼出版本 | §9.2                                                                                                                                      |

### 1.5 版本、兼容性与 Schema 生成器

| 事实                                                                                 | 位置                                                                 |
| ------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| `manifest_version` 是整数，只接受 2；没有 minor，无法表达兼容更新                    | `M/lib.rs:39-69`；`G/main.rs:102`                                    |
| serde 遇到未知的 `SupportedPlatform` 或 parser `type` 时，整份 manifest 反序列化失败 | `M/lib.rs:107-120`、`:298-332`                                       |
| 未知结构字段被静默忽略（未开 `deny_unknown_fields`），模板侧也一样                   | `M/lib.rs` 全部 struct                                               |
| 目前没有消费者读取 `resources.json`；现有的 updater 仍然读 `version.json`            | `D/:350-357`                                                         |
| 生成器把**任何**名为 `manifest_version` 的字段改写成字面量 `2`                       | `G/main.rs:102`                                                      |
| 生成器删除 Schema 中**所有**名为 `format` 的键，包括将来可能出现的同名属性定义       | `G/main.rs:157`                                                      |
| `                                                                                    | null`断言发生在全局 lossless-float 重映射之后，挡不住将来新增的`f64` | `G/main.rs:51`、`:249` |

### 1.6 工具链

| 事实                                                                                      | 位置                                                       |
| ----------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| 本机已装 `wasm32-unknown-unknown` target 和 `wasm-bindgen` CLI 0.2.114；没有装 `wasm-opt` | `rustup target list --installed`；`wasm-bindgen --version` |
| Node ≥ 24.2，TS 源码直接运行，包以 `.ts` 形式导出，仓库目前没有构建步骤                   | `package.json`；`AGENTS.md` §运行时                        |

---

## 2. 优先级评估

### 2.1 修复与新方向

按以下维度排序：Critical 与安全优先；后续工作依赖的前置项优先；必须在首次发布时就位的项优先（发布后再加就是破坏性变更）。

| 优先级 | 项                                                                                                   | 来源                                 | 理由                                                                                      | 阶段    |
| ------ | ---------------------------------------------------------------------------------------------------- | ------------------------------------ | ----------------------------------------------------------------------------------------- | ------- |
| **P0** | 协议修订：信任模型与 `ChecksumKind`、模板值分类、regex 方言、parser 精确语义、错误码、消费者边界     | Critical #2、Major #3/#9/#10、O-1    | 后面每张卡都以它为准；只改文档                                                            | Phase 0 |
| **P0** | `manifest_version` 改为 `major.minor`；消费者对新 minor 的宽松解码                                   | Major #8、O-7                        | 首个发布的消费者必须能接受以后的 minor，否则 minor 就没有意义；发布前改动是免费的（O-11） | Phase 0 |
| **P0** | 一致性测试集                                                                                         | Suggestion #12                       | 是"单一语义"唯一可以机械验证的证据                                                        | Phase 0 |
| **P0** | 契约加固：`group` 上限、版本常量改为生成、生成器断言收窄、模板侧 `deny_unknown_fields`               | Minor #9/#10                         | 新 crate 会导入这些类型，先加固能避免把漂移带过去                                         | Phase 0 |
| **P1** | `manifest-parser`（含 `github-release-assets` 精确匹配、BSD 标签核对、`trim` 语义、`header` parser） | 维护者提议、Major #4、Minor #11、O-1 | 叶子 crate；clash-nyanpasu 的 Rust updater 可以直接用                                     | Phase A |
| **P1** | `validate_manifest` → `ValidatedManifest`                                                            | Critical #1                          | 任何消费端都需要                                                                          | Phase B |
| **P2** | 模板引擎与 resolver（值分类、URL 校验、`ChecksumKind`）                                              | Major #3、O-1                        | 依赖 P0 和 P1                                                                             | Phase B |
| **P2** | wasm 绑定、`@manifest/runtime`、构建期下载校验胶水脚本                                               | O-6                                  | 依赖 core                                                                                 | Phase C |
| **P3** | 编译器移植、快照边界、模板可解析性的编译期检查、Clash Rust alpha 的版本来源                          | Major #5/#6/#7                       | 现在由上游 TS 生成器承担，工作量最大                                                      | Phase D |
| 不做   | i18n 键约定、mirror 元数据、平台映射 helper、npm 发布构建、签名的实现                                | kimi 意见、O-2                       | 不影响语义的统一                                                                          | §12     |

### 2.2 为什么不先做 wasm

wasm 的价值在于"Node 和 Rust 共享一份实现"，前提是这份实现的语义已经正确并且冻结。按"Phase 0 → A → B → C"的顺序做，wasm 只是一层薄适配，在最后接入。

---

## 3. 目标与成功判据

| #   | 判据                                                                                                                                                                             | 验证方式                                                                                                                                                                     |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G1  | `manifest-parser` 与 `manifest-core` 不做 IO                                                                                                                                     | `cargo tree -p manifest-core -e normal --prefix none` 中不含 `reqwest`、`tokio`、`hyper`、`ureq`；`rg 'std::(net\|fs)' crates/manifest-parser crates/manifest-core` 退出码 1 |
| G2  | 依赖方向是 `manifest-parser` ← `manifest-metadata` ← `manifest-core` ← `manifest-wasm`                                                                                           | `cargo tree -p manifest-parser -e normal` 中不含 `manifest-metadata`                                                                                                         |
| G3  | 一致性测试集在 native Rust 与 Node（wasm）两端全部通过，用例清单相同                                                                                                             | `cargo test -p manifest-core --test conformance`；`pnpm test`；`pnpm check:conformance`                                                                                      |
| G4  | §1.2、§1.3 中复现过的每个缺陷都有用例，并断言修正后的结果                                                                                                                        | 测试集 README 的"缺陷 ↔ 用例"表；临时退回旧语义时，对应用例会失败                                                                                                            |
| G5  | 对快照中每个 resource × platform × 默认 variant，Rust resolver 生成的 `canonical_url`、`download_url` 与参考实现逐字节相同                                                       | `conformance/resolve/reference-golden.json` 的差分测试                                                                                                                       |
| G6  | `validate_manifest` 对适用于编译产物的每条不变量都有一正一反两个用例                                                                                                             | `cargo test -p manifest-core validate::`                                                                                                                                     |
| G7  | 生成物只出现预期内的变化：`manifest_version` 变为 `"2.0"` 及其类型、`group` 上限、版本常量、模板侧 `additionalProperties: false`、`header` parser。`pnpm check:generated` 无差异 | `pnpm check:generated`；PR 描述列出差异                                                                                                                                      |
| G8  | 一份 `minor` 比已知值更新、且带未知 platform key 和未知 parser type 的 manifest 能被解析；只有受影响的产物报 `resolve.unsupported_*`，其余产物正常                               | `conformance/compat/` 用例                                                                                                                                                   |
| G9  | 每个产物的 `ChecksumKind` 不联网就能得出；默认策略只接受 `authoritative`                                                                                                         | `artifact_checksum_kind` 的用例；胶水脚本对 `parsed` 产物默认失败的测试                                                                                                      |
| G10 | Node 包只通过 `@manifest/runtime` 调用语义逻辑                                                                                                                                   | `rg 'parseRemote\|renderTemplate\|tag_key' packages --glob '!manifest-runtime/**' --glob '!**/*_test.ts'` 退出码 1                                                           |
| G11 | wasm 产物大小与 Node 初始化耗时有记录，并据此定下预算                                                                                                                            | C1 卡的 PR 描述；O-8                                                                                                                                                         |

---

## 4. 目标包与依赖规则

### 4.1 Phase C 结束时的包

| 包                                 | 职责                                                                                                                                                                            | 允许依赖                                                               |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `crates/manifest-parser`           | `ChecksumAlgorithm`、`ResourceChecksum`、`RemoteVersionParser`、`RemoteChecksumParser` 的定义（wire 类型）；`VersionLabel`；`RemoteResponse`；parser 的校验与执行；`ParseError` | serde、schemars、specta、regex、HTML tokenizer（A4 定）、base64、snafu |
| `crates/manifest-metadata`         | manifest 与模板 DTO；`ManifestVersion`（`major.minor`）、`FiniteF64`；对新 minor 的宽松解码                                                                                     | `manifest-parser` 与现有依赖                                           |
| `crates/manifest-core`             | `validate_manifest`、模板引擎、variant 选择、版本计划与下载计划、`ChecksumKind`、校验策略                                                                                       | `manifest-parser`、`manifest-metadata`、`url`、snafu                   |
| `crates/manifest-wasm`             | `wasm-bindgen` 薄适配层                                                                                                                                                         | `manifest-core`、`wasm-bindgen`、`serde-wasm-bindgen`                  |
| `crates/manifest-schema-generator` | 类型来源加上 `manifest-parser` 与 core 的计划类型                                                                                                                               | 现有依赖加 `manifest-parser`、`manifest-core`                          |
| `packages/manifest-runtime`        | 加载 wasm、类型化的 facade、错误对象                                                                                                                                            | `@manifest/metadata`（只用类型）                                       |
| `packages/manifest-downloader`     | 构建期胶水脚本：下载并校验资源；HTTP、重试、文件落盘、校验策略的命令行开关                                                                                                      | `@manifest/runtime`、`@manifest/metadata`                              |
| `packages/manifest-generator`      | Phase D：抓取快照，经 wasm 编译                                                                                                                                                 | 同上                                                                   |

### 4.2 依赖规则

| 规则                                                             | 检查                                                             |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| parser 不依赖 metadata 和 core                                   | `cargo tree -p manifest-parser -e normal`                        |
| parser 与 core 不依赖任何 HTTP 或异步运行时                      | G1                                                               |
| metadata 不依赖 core                                             | `cargo tree -p manifest-metadata -e normal`                      |
| 每个类型只在一处定义，搬走后不在旧位置留 re-export（rust.md §4） | `rg 'pub use manifest_parser' crates/manifest-metadata` 退出码 1 |
| TS 包不能 import 另一个包的 `src/` 内部路径                      | `rg "from \"@manifest/[a-z-]+/src" packages` 退出码 1            |

---

## 5. Phase 0：协议冻结

S0a 卡把本节内容写进 `D/`。本节是规范性的。

### 5.1 信任模型与 `ChecksumKind`

1. **信任锚点是签过名的 manifest。** 签名只覆盖 manifest 本身，所以只有 manifest 内嵌的 checksum 是权威的（方案为 minisign，接口见 §9.1，O-2）。
2. **`ChecksumKind` 有四种：** wire 名称固定为小写 snake_case：`authoritative`、`parsed`、`snapshot`、`none`（Rust 用 `#[serde(rename_all = "snake_case")]`）。

   | kind            | 来源                                                                                                 | 默认策略                           |
   | --------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------- |
   | `authoritative` | static 产物的 release 中内嵌的 `checksum`                                                            | 接受                               |
   | `parsed`        | remote 产物按 parser 规则从远端响应解析出的 checksum（body 或 header；不论走 canonical 还是 mirror） | 拒绝，需要宿主显式开启             |
   | `snapshot`      | remote 产物 `fallback` 中内嵌的 checksum，只在解析出的版本恰好等于 `fallback.version` 时适用         | 拒绝，需要宿主显式开启（离线策略） |
   | `none`          | 没有任何 checksum                                                                                    | 一律拒绝，不提供开关               |

   md5 不改变 kind；宿主可以另外按算法拒绝。

   推荐展示方式如下；具体 UI 文案不在本设计范围内（§12）。展示建议不代替策略决定。

   | kind            | 推荐展示                                                                 |
   | --------------- | ------------------------------------------------------------------------ |
   | `authoritative` | 无警告                                                                   |
   | `parsed`        | 警告：完整性以更新时获取的值为依据，而不是签名 manifest 中的值           |
   | `snapshot`      | 警告：这是内嵌的离线快照，仅在解析出的版本等于 `fallback.version` 时适用 |
   | `none`          | 硬性阻止；任何校验策略都不能接受                                         |

   宿主必须在 kind 旁同时暴露有效策略决定：用宿主的 `VerifyPolicy` 调用 `check_policy` 得到接受，或拒绝及原因。kind 本身不能决定最终结果。

3. **kind 静态可推。** `artifact_checksum_kind(&ResourceArtifactTemplate) -> ChecksumKind` 只看 manifest：static 的 latest release 有 checksum 时为 `authoritative`；latest 没有 checksum 时为 `none`；remote 且声明了 `checksum` 规则时为 `parsed`；remote 且没有规则时为 `none`。未知 version 或 checksum parser 的产物为 `none`。前端（经 clash-nyanpasu 后端）在用户选择、更新 alpha 这类内核**之前**就能拿到 kind 并提示；提供按 resource 与平台批量列出每个 variant kind 的 helper（§7.3）。静态推导用于选择前展示；下载计划和最终结果携带执行时适用的 kind（包括显式离线 fallback 适用的 `snapshot`），宿主同时暴露该 kind 的有效策略决定。
4. **只有 parser 规则能产生 `parsed`。** 宿主不得自行从响应头（`Digest`、`Repr-Digest`、`ETag`、`x-*-checksum` 等）或任何未声明的来源推断 checksum。要用 header，必须在 manifest 里写 `header` parser 规则（§5.3）。
5. **mirror 只负责传输**，不影响 kind。下载计划同时给出 `canonical_url` 和可选的 `mirror_url`，宿主把实际使用的通道交回 core，只用于诊断和日志。
6. **宿主策略**（胶水脚本的默认值）：只接受 `authoritative`。`--allow-parsed`、`--allow-snapshot` 分别放开对应 kind，并输出警告。`none` 不被任何校验策略接受。不论哪种 kind，校验失败都在解压或替换之前终止。

### 5.2 模板值分类

现有 wire 格式不变，只收紧渲染语义。

| 类别       | 来源                                                                 | 递归展开                         | 约束                                                                        |
| ---------- | -------------------------------------------------------------------- | -------------------------------- | --------------------------------------------------------------------------- |
| `Authored` | manifest 中的 `source`、registry `variables`、remote `source`        | 是（例：`release: "{version}"`） | 检测环；未知变量报错；嵌套深度和展开后长度设上限（§5.5）                    |
| `Literal`  | 远端解析出的 version、宿主传入的 `source` 与 `version`、URL 组件变量 | 否                               | `version` 与宿主变量必须满足 `VersionLabel`；URL 组件变量只用于 mirror 模板 |

- **`VersionLabel` 字符集：** `^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$`。快照中出现的 7 个版本全部满足（2026-11-06 已核对）。
- **不做百分号编码。** 快照中所有合法产物的 URL 与参考实现逐字节相同（G5）。
- **最终 URL 的校验：** 用 `url` crate（WHATWG 语义）解析；协议必须是 http(s)；不能带 userinfo 和 fragment。

### 5.3 parser 精确语义

parser 的输入是 `RemoteResponse { body: &str, headers: &[(String, String)] }`；header 名称不区分大小写。headers 的每项是 HTTP 客户端交付的字段值，可以是一行，也可以是按 RFC 9110 §5.3 合并的逗号列表。body 或单个收到的 header 值超过上限（§5.5）直接报 `parser.body_too_large`，字段为 `{ limit, actual }`。

| parser                  | 语义                                                                                                                                                                                                                                                                                                                                   |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `text`                  | `trim` 默认 `true`：先去掉首尾空白再校验；为 `false` 时 body 原样校验。结果必须满足 `VersionLabel`                                                                                                                                                                                                                                     |
| `regex`                 | 方言是 Rust `regex` crate 语法，不带 flag，可以用内联 `(?i)` 等。取第一个匹配的 `group`（默认 1，0 表示整个匹配），去掉首尾空白后必须满足 `VersionLabel`。校验时检查：pattern 能编译、编译大小不超上限、`group` 小于捕获组数量                                                                                                         |
| `checksum`              | body 去掉首尾空白后按 `ResourceChecksum` 解析                                                                                                                                                                                                                                                                                          |
| `gnu`                   | 按行切分（`\n` 或 `\r\n`），行的格式为 `<hex>[ \t]+[* ]?<name>[ \t]*`；名称完全相等；hex 长度必须等于该算法的长度。同名多行、digest 相同时取一条；digest 不同报 `ambiguous`；没有匹配报 `not_found`                                                                                                                                    |
| `bsd`                   | 行的格式为 `<LABEL> (<name>) = <hex>`；`LABEL` 不区分大小写，必须等于配置的算法，不等的行跳过；名称完全相等；冲突报 `ambiguous`                                                                                                                                                                                                        |
| `github-release-assets` | 把 HTML 切成资产条目：名称取自下载链接 `href` 的最后一个路径段（百分号解码后），digest 只从同一条目内读取。名称必须完全相等；没有匹配报 `not_found`；匹配条目没有 digest 报 `digest_missing`；同名条目 digest 冲突报 `ambiguous`                                                                                                       |
| `header`（新增）        | `{ type: "header", name, algorithm, encoding: "hex" \| "base64" }`：收集指定名称的所有字段值（名称大小写不敏感），每项按 `,` 拆分，裁剪元素两端的 SP / HTAB 并忽略空元素（RFC 9110 §5.6.1）。没有元素报 `not_found`（包括只有空元素的值）；元素完全相同则接受，不同报 `ambiguous`；`base64` 先解码再转成小写 hex；长度必须等于算法长度 |

合法的 hex / base64 digest 不含逗号，单个合法值不因拆分而改变。8 KiB 单值上限在拆分前逐项检查，不按元素计算。header 的 base64 非法时报 `parser.invalid_encoding`；hex 含非十六进制字符或字符数为奇数，以及解码后 digest 长度不等于算法长度，均报 `parser.invalid_checksum`。单个 header 值超过单值上限（默认 8 KiB，§5.5；宿主调低后以生效值为准）时优先报 `parser.body_too_large`（`{ limit, actual }`）；此码同时适用于 body 和单个 header 值，不新增错误码。

version parser 不支持 header，需求出现时再按 minor 增加。

### 5.4 版本与兼容策略（参考 Docker）

1. **格式：** `manifest_version` 是字符串 `"<major>.<minor>"`，如 `"2.0"`。
   - TS 类型：`` `2.${number}` ``。
   - Schema：`pattern: "^2\\.(0|[1-9][0-9]*)$"`。
   - Rust：`ManifestVersion { minor: u32 }`，major 是常量 2。
   - 生产者写出当前的 `CURRENT_MINOR`（当前为 0）。minor 不允许前导零；`"2.07"` 报 `manifest.invalid_version_format`。
2. **消费规则：** major 不等于 2 时拒绝（`manifest.unsupported_major`）。minor 任意，都接受。minor 大于消费者的 `CURRENT_MINOR` 时，结果中带 `newer_minor: true`，供诊断。
3. **只升 minor 的兼容更新**：旧消费者要么能忽略新增内容，要么只对受影响的那个产物报明确的不支持错误，不影响其他产物。
   - 新增可选字段：旧消费者忽略。
   - 新增 platform：旧消费者跳过未知的 platform key。
   - 新增 parser type 或 checksum parser type：旧消费者解码为"不支持"，只在解析该产物时报 `resolve.unsupported_parser`，结构字段 `parser_type` 保留原始未知的 `type` tag。旧消费者对这类产物推出的 kind 为 `none`，任何校验策略都不能接受，因此该产物不可用，而不是静默跳过校验。
4. **必须升 major 的变更：** 新增必填字段；改变已有字段或 parser 的语义；删除字段；把已有产物换成旧消费者不认识的 parser type（这会让旧消费者失去原有的能力）。major 升级沿用 v1 → v2 的双文件过渡模式（`D/:337-348`）。
5. **宽松只在编译产物的解码侧。** 模板侧严格（`deny_unknown_fields`，O-5），用来拦住作者的拼写错误。生成的 TS 与 JSON Schema 描述当前 minor 的精确契约，不包含"未知"变体。消费者不能只因为旧版 Schema 校验失败就拒绝新 minor 的 manifest，消费侧校验以 `validate_manifest` 为准。Schema 的 `$id` 带 `major.minor`。
6. **发布前冻结版本号（O-11）：** `@manifest/*` 或 crate 首次发布之前，版本保持 `"2.0"`，这期间的契约改动（包括本 spec 的全部改动）都并入 2.0，不升 minor。发布之后才按第 3、4 条升版本。

### 5.5 资源上限

上限是 core 的常量，宿主可以调低，不能调高。

| 项                                           | 上限  |
| -------------------------------------------- | ----- |
| parser body                                  | 2 MiB |
| 单个 header 值                               | 8 KiB |
| regex pattern 长度                           | 1 KiB |
| regex 编译大小（`RegexBuilder::size_limit`） | 1 MiB |
| 模板嵌套深度                                 | 8     |
| 单次渲染输出                                 | 8 KiB |

body 和单个 header 值的大小按 UTF-8 字节数计算；超限均报 `parser.body_too_large`，`limit` 为有效字节上限，`actual` 为实际字节数。

具体数值在 A2 卡用真实页面大小复核，改动记进 §11.2。

### 5.6 错误码

错误是带结构字段的类型化枚举（rust.md §7.2，用 snafu）。每个变体有稳定的 `code()`；跨语言只靠 `code` 加结构字段识别，不解析 `message`。

| 前缀        | 码                                                                                                          | Fields                            |
| ----------- | ----------------------------------------------------------------------------------------------------------- | --------------------------------- |
| `manifest.` | `unsupported_major`                                                                                         | `{ major }`                       |
| `manifest.` | `invalid_version_format`                                                                                    | `{}`                              |
| `parser.`   | `body_too_large`                                                                                            | `{ limit, actual }`               |
| `parser.`   | `ambiguous`                                                                                                 | `{ name }`                        |
| `parser.`   | `group_out_of_range`                                                                                        | `{ group, groups }`               |
| `parser.`   | `not_found`, `digest_missing`, `invalid_version`, `invalid_checksum`, `invalid_encoding`, `invalid_pattern` | 结构字段待对应实现卡细化          |
| `template.` | `unknown_variable`, `cycle`, `depth_exceeded`, `output_too_large`, `invalid_literal`                        | 见下方 B2 渲染错误表              |
| `url.`      | `invalid`, `unsupported_scheme`, `forbidden_component`                                                      | 见下方 B2 渲染错误表              |
| `resolve.`  | `unknown_resource`, `unsupported_platform`, `variant_not_found`                                             | 见下方 B3 错误表                  |
| `resolve.`  | `tags_rejected`, `selection_conflict`, `default_variant_missing`                                            | 见下方 B3 错误表                  |
| `resolve.`  | `unknown_mirror`, `mirror_not_allowed`, `snapshot_unavailable`                                              | 见下方 B3 错误表                  |
| `resolve.`  | `version_required`, `version_not_in_history`, `unsupported_parser`                                          | 见下方 B3 错误表                  |
| `verify.`   | `kind_rejected`                                                                                             | `{ kind }`                        |
| `verify.`   | `digest_mismatch`                                                                                           | `{ algorithm, expected, actual }` |
| `validate.` | 完整清单见下方 B1 校验错误表                                                                                | —                                 |

错误都带 `path`，统一使用 dotted locator，根为输入文档、宿主参数 `arg` 或 parser 响应 `response`：以 `.` 连接字段或对象键，数组下标写作 `[<i>]`，例如 `manifest_version`、`resources.<id>.variants[<i>].tags`。宿主传入参数使用同一形式的 `arg.<name>`，例如 `arg.version`；parser 响应错误使用 `response.body` 或 `response.headers.<小写名称>`；不使用裸参数名或其他路径语法。一致性运行器对声明的 `path` 做精确字符串比较。

parser 规则错误使用相对于规则的定位：`invalid_pattern` 为 `parser.pattern`，`group_out_of_range` 为 `parser.group`，`resolve.unsupported_parser` 为 `parser.type`；B1 校验完整 manifest 时加上规则所在位置的前缀，例如 `resources.<id>.variants[<i>].artifacts.<platform>.version.version.parser.pattern`。两个 `version` 分别是产物的版本策略和 remote 策略里的版本请求；不得省略 wire 层级。

A4 细化：GitHub 的 `not_found`、`digest_missing`、`ambiguous` 字段均为 `{ name }`，名称为请求的 artifact 原值；`invalid_checksum` 字段为 `{}`，包括不支持的算法、非法 hex 和算法长度错误。上述错误的 `path` 均为 `response.body`。匹配条目没有 digest 时拒绝，即使另有同名条目带 digest；未匹配条目的畸形 digest 不参与校验。

A2–A5 细化：version 的 `not_found`、`invalid_version` 字段为 `{}`；GNU / BSD 的 `not_found`、`ambiguous` 字段为 `{ name }`，保留 artifact 原值。`invalid_checksum`、`invalid_encoding` 字段为 `{}`；`invalid_pattern` 在 pattern 字节数超限时为 `{ limit, actual }`，语法或编译大小错误时为 `{}`。header 按 §5.3 拆分客户端交付的逗号列表，忽略空元素；只有空元素与缺失一样报 `not_found`。header 的 `not_found`、`ambiguous` 字段为 `{ name }`，保留规则声明的大小写；其 `body_too_large` 字段为 `{ limit, actual }`。所有 header 响应错误的路径为 `response.headers.<ASCII 小写名称>`，其余响应错误为 `response.body`；规则错误路径遵循上面的 parser 规则定位。

消费者按 `code` 与结构字段进行程序化匹配，不解析 `message`。`parser.ambiguous` 的 `name` 是请求的 artifact 文件名或 header 规则中的名称；`groups` 是包含 group 0 的捕获组总数。

#### B1 校验错误

以下十行对应 §7.1 的十组不变量，每组一个稳定码。`R` 表示
`resources.<id>`，`V` 表示 `R.variants[<i>]`，`A` 表示
`V.artifacts.<platform>`；这些缩写只用于表格，实际 `path()` 返回完整 locator。

| 不变量             | 完整 code                    | fields                          | path                                                                                          |
| ------------------ | ---------------------------- | ------------------------------- | --------------------------------------------------------------------------------------------- |
| ID 合法且唯一      | `validate.ids`               | `{ kind, id, reason }`          | `R.tags[i].id`、`V.id`、`V.tags[i].id`                                                        |
| tag key 规范拼接   | `validate.tag_key`           | `{ expected, actual }`          | `V.tag_key`                                                                                   |
| 展开 tag 一致      | `validate.expanded_tag`      | `{ id, reason }`                | `V.tags[i]`                                                                                   |
| group 互斥         | `validate.tag_group`         | `{ group, tags }`               | `V.tags`                                                                                      |
| 精确 tag 组合唯一  | `validate.tag_combination`   | `{ tag_key, first_variant }`    | `V.tags`                                                                                      |
| 平台默认有效且完整 | `validate.default_variant`   | `{ platform, variant, reason }` | `R.default_variants.<platform>`                                                               |
| 引用存在           | `validate.reference`         | `{ kind, id }`                  | `R.registry`、`A.version.<version\|checksum>.resource.registry`、`mirrors.<id>.registries[i]` |
| static 历史有效    | `validate.static_history`    | `{ version, reason }`           | `A.version.latest`、`A.version.releases[i].version`                                           |
| 已知 parser 可编译 | `validate.parser_compile`    | `{ reason, details }`           | `A.version.version.parser.<pattern\|group>`、`A.version.checksum.parser.name`                 |
| 模板依赖完整       | `validate.template_variable` | `{ variable }`                  | 实际包含缺失变量引用的 artifact、source、variables 或 url_template 字段                       |

`ids.kind` 为 `tag` / `variant`，`reason` 为 `invalid` / `duplicate`；
`expanded_tag.reason` 为 `unknown` / `mismatch`（对比 id、name、group、description，
缺省与空字符串不同）。`tag_group.tags` 为按码点排序的不同 ID；`first_variant`
为第一个相同集合的零起始下标。`tag_combination.tag_key` 是去重后集合按码点排序、
以 `+` 拼接的规范 key，不是 variant 输入的 `tag_key`。`default_variant.reason` 为 `missing` / `unknown` /
`unsupported_platform`，缺默认时 `variant` 为 JSON null，否则为引用的 ID。
`reference.kind` 当前只有 `registry`：wire 里没有具名 mirror 引用，只有 mirror 对
registry 的允许列表；宿主选择的 mirror 在 B3 校验，不虚构 manifest 字段。
`static_history.reason` 为 `latest_missing` / `duplicate`。

版本 parser 使用 `compile(&Limits::default())`，错误统一包为
`validate.parser_compile`，`reason` 保留原 `ParseError::code()`，`details`
完整保留原 `fields()`，path 为请求位置加原规则相对路径。checksum parser 当前没有
compile API：checksum / GNU / BSD / GitHub 的配置由类型表达，预检不会读响应；
header 额外检查 RFC 9110 非空 token 名称，失败的 `reason` 为 `invalid_header_name`，
`details` 为 `{ name }`。两种 `Unsupported` 都不产生校验错误。

`ValidationErrors.errors()` 返回全部错误；同一 locator 的多个独立违规仍分别返回。
同一模板字段在多个平台 / release 作用域内缺少同一变量时只返回一次相同错误。

#### B2 渲染错误

| 完整 code                        | fields              | path                                                               |
| -------------------------------- | ------------------- | ------------------------------------------------------------------ |
| `template.unknown_variable`      | `{ variable }`      | 包含引用的模板字段                                                 |
| `template.cycle`                 | `{ variables }`     | 包含闭环引用的模板字段                                             |
| `template.depth_exceeded`        | `{ limit, actual }` | 包含超限引用的模板字段                                             |
| `template.output_too_large`      | `{ limit, actual }` | 超限输出所属模板字段                                               |
| `template.invalid_literal`       | `{}`                | `arg.version`、`arg.source.<key>`、`A.version.releases[i].version` |
| `url.invalid`                    | `{}`                | `registries.<id>.url_template`                                     |
| `url.unsupported_scheme`         | `{ scheme }`        | `registries.<id>.url_template`                                     |
| `url.forbidden_component`        | `{ component }`     | `registries.<id>.url_template`                                     |
| `resolve.version_required`       | `{}`                | `arg.version`                                                      |
| `resolve.version_not_in_history` | `{ version }`       | `arg.version`                                                      |

`A` 沿用 B1 表的缩写。`variables` 为从根开始的 DFS 变量栈，包含末尾重复的闭环变量；
`scheme` 为 URL 解析器规范化的小写协议名，不带冒号；`component` 为 `userinfo` 或 `fragment`。
URL 错误顺序为解析失败、协议、userinfo、fragment；空 userinfo（`https://@host/`）
也拒绝，通过 `url` 的 `EmbeddedCredentials` 诊断识别，不能在规范化后丢失检查。
空 fragment 同样拒绝；作者显式写出的 query 允许。

渲染根深度为 0，每进入一个 Authored 变量值深度加 1，Literal 不增加深度；
检测到重复 DFS 变量时优先报 cycle。`depth_exceeded.actual` 是试图进入的深度。
每层输出都在追加前检查 UTF-8 字节数，`output_too_large.actual` 是首次超限追加
将得到的字节数（当前前缀加待追加片段），并非继续展开剩余模板后的总大小；不会分配超限输出。
没有重新扫描拼接后的结果，所以 Literal 或跨替换边界形成的 `{x}` 都不再解释。
未知变量、环及深度错误定位到正在解释的模板字段；输出错误定位到正在累积输出的字段。
通用 `render_template` 没有 manifest locator，统一使用调用者给定的 path。

所有宿主 source 值（包括未使用或被内置值覆盖的项）与宿主 version 在合并之前
按 `VersionLabel` 校验，不 trim、不接受空值、不另设更宽松的字符集。
通用 `TemplateValue::Literal` 本身不验证字符集，允许未来 mirror 的 URL 组件包含
URL 标点；宿主输入必须通过 artifact 渲染入口校验，不能直接作为未检查的通用 Literal 使用。
默认 static latest 的值在渲染时校验，错误定位到实际 release 的 version 字段；
宿主传入的 static 历史选择或 remote parser 结果都定位到 `arg.version`。
remote 缺版本与 static 不在历史中分别使用上表两个 resolve 码，供 B3 复用。

#### B3 选择、计划与 VerifyError

下表补全 B3 的稳定结构字段；计划转发 B2 `RenderError` 或 parser `ParseError` 时，
保留原 code/path/fields。`resolve.unsupported_parser` 的 path 保持 `parser.type`，字段保留原未知 tag。
`unknown_registry` 留给接受自定义 registry 的后续 API；B3 只消费已验证 manifest 中的命名引用。

| code                              | fields                            | path                                         |
| --------------------------------- | --------------------------------- | -------------------------------------------- |
| `resolve.unknown_resource`        | `{ resource }`                    | `arg.resource`                               |
| `resolve.unsupported_platform`    | `{ resource, platform }`          | `arg.platform`                               |
| `resolve.variant_not_found`       | `{ variant, platform }`           | `arg.variant_id`                             |
| `resolve.tags_rejected`           | `{ tags, reason }`                | `arg.tags`                                   |
| `resolve.selection_conflict`      | `{ variant, tags }`               | `arg.variant`                                |
| `resolve.default_variant_missing` | `{ resource, platform }`          | `resources.<id>.default_variants.<platform>` |
| `resolve.unknown_mirror`          | `{ mirror }`                      | `arg.mirror`                                 |
| `resolve.mirror_not_allowed`      | `{ mirror, registry }`            | `arg.mirror`                                 |
| `resolve.snapshot_unavailable`    | `{ version, fallback_version }`   | `arg.version`                                |
| `verify.kind_rejected`            | `{ kind }`                        | `arg.kind`                                   |
| `verify.digest_mismatch`          | `{ algorithm, expected, actual }` | `arg.actual`                                 |

`SelectTags.reason` 固定为 `empty`、`duplicate`、`not_found`，tags 保留宿主原顺序与重复项。
`SelectFallback.fallback_version` 缺少 fallback 时为 null，否则为 manifest 中的原版本。
`VerifyError::CheckKind` 对应 kind_rejected；`VerifyError::CompareDigest` 对应 digest_mismatch。

### 5.7 消费者边界

| 消费者                                       | 接入方式                                                                              |
| -------------------------------------------- | ------------------------------------------------------------------------------------- |
| Rust 宿主（clash-nyanpasu 后端）             | 直接依赖 `manifest-core`；HTTP 由宿主实现；`ChecksumKind` 经 IPC 给前端，前端据此提示 |
| Node 构建期胶水脚本（downloader、generator） | `@manifest/runtime`（wasm）                                                           |
| Tauri webview                                | 只展示后端给的结果与 kind；不加载 wasm                                                |
| 纯浏览器                                     | 不支持                                                                                |

---

## 6. Phase A：`manifest-parser`

### 6.1 A1 搬迁（只搬代码，行为不变）

- `M/lib.rs:122-260`（`ChecksumAlgorithm`、`ParseChecksumError`、`ResourceChecksum`）和 `:297-332`（两个 parser 枚举）搬进 `crates/manifest-parser`，metadata 改为依赖它，不在旧位置留 re-export。
- 生成物必须**无差异**。现有的 checksum 单元测试随类型一起搬走。

### 6.2 A2 version parser

- `VersionLabel` 是带私有字段的 newtype，实现 `FromStr` 和 `Display`（rust.md §5.2、§5.3）。
- `RemoteVersionParser::compile(&self, &Limits) -> Result<CompiledVersionParser, ParseError>`；`CompiledVersionParser::parse(&self, &RemoteResponse) -> Result<VersionLabel, ParseError>` 是纯函数，标 `#[must_use]`。
- 编译期校验和运行期执行用同一个 `compile`，保证"校验通过就一定能执行"。

### 6.3 A3 checksum parser（`checksum`、`gnu`、`bsd`）

- `RemoteChecksumParser::parse(&self, &RemoteResponse, artifact: &str, &Limits) -> Result<ResourceChecksum, ParseError>`。
- 按 §5.3 手写逐行解析，不为每次调用编译正则。

### 6.4 A4 `github-release-assets`

1. 先从真实的 `expanded_assets` 页面采集 fixture，至少包括：一个带 digest 的资产、一个不带 digest 的资产、两个名称有前缀关系的资产、名称中带 `+` 或空格的资产。
2. 按 wasm 体积增量与 fixture 正确性，在轻量 tokenizer 和完整 DOM 解析器之间选一个（O-3）。
3. 名称取 `href` 的最后一个路径段，而不是显示文本。

### 6.5 A5 `header` parser

- 新增枚举变体 `Header { name, algorithm, encoding }`，`encoding` 是 `Hex | Base64` 枚举（不用 `bool`，rust.md §5.2）。
- 这是 2.0 内的契约新增（O-11），生成物出现新变体，属于 G7 的预期变化。

### 6.6 规则落点

| rust.md 规则                            | 本阶段落点                                                                                                   |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| §5.2 newtype、用枚举代替含义不清的 bool | `VersionLabel`；`HeaderEncoding`                                                                             |
| §5.3 用标准 trait                       | `FromStr`/`Display`/`TryFrom<&str>`                                                                          |
| §5.5 `#[non_exhaustive]` 只用于外部协议 | parser 枚举加 `#[non_exhaustive]`：下游 clash-nyanpasu 跨仓库匹配这些枚举，而协议会按 minor 新增变体（§5.4） |
| §6 wire 名称                            | 搬迁不改任何 serde 名称                                                                                      |
| §7.2 snafu、结构字段                    | `ParseError`，带 `code()`                                                                                    |

---

## 7. Phase B：`manifest-core`

### 7.1 B1 `validate_manifest`

- `validate_manifest(raw: ResourceManifest) -> Result<ValidatedManifest, ValidationErrors>`，一次收集所有违规，不在第一条就停。
- `ValidatedManifest` 字段私有，只能经这个函数构造，提供只读访问。
- 不变量（适用于编译产物）：
  - tag 与 variant 的 ID 合法且唯一；
  - `tag_key` 等于对排序后 tag 的规范拼接；
  - 展开后的 tag 与 resource 级 tag 目录一致；
  - 同一非空 group 内的 tag 互斥；
  - 精确 tag 组合唯一；
  - `default_variants` 指向的 variant 存在且支持该平台；有多个 variant 支持某平台时必须配置默认；
  - registry 和 mirror 的引用都存在；
  - static 历史中 `latest` 必须在 `releases` 里，且版本不重复；
  - 每个已知 parser 都能 `compile`；未知 parser 不报校验错误，在解析该产物时才报（§5.4 第 3 条）；
  - 模板变量可解析（只检查依赖是否完整）。

B1 的 ID 规则与只读参考实现对齐并在此冻结：tag ID 为
`^[A-Za-z0-9][A-Za-z0-9._:-]*$`，variant ID 为
`^[A-Za-z0-9][A-Za-z0-9._:+-]*$`。大小写敏感，不 trim；tag 的 `+` 保留作分隔符。
resource 目录内 tag ID 唯一，每个 variant 内 tag ID 唯一，resource 内 variant ID 唯一。
`tag_key` 是所有 tag ID 按 Unicode 码点排序后用 `+` 拼接（空列表为 `""`）；
组合唯一性比较去重后的精确集合，不相信输入的 `tag_key`。group 只有缺省与 `""`
不参与互斥，空白字符串也是非空 group。重复 ID、伪造 key 和重复组合各自独立报告。

模板依赖检查识别 `{[A-Za-z0-9_]+}`，对实例化的 download、remote version、
remote checksum 作用域检查合并后的有效值（含未被模板使用的有效 source 值）及模板。
优先级为 registry variables < resource source < variant source < artifact source <
release source / remote source < 内置变量；静态历史逐条检查，remote 检查一般运行时
与 fallback 作用域，fallback source 不泄漏到版本请求或一般运行时作用域。
download 的 artifact 名模板只使用 source 与内置变量；registry variables 在产物名
渲染后才合并。除 §11.1 列出的有意偏差外，作用域与参考实现对齐。
内置来源为 `resource`、`resource_id`、`variant`、`variant_id`、`tag_key`、`platform`、
`version`；`version` 作为运行时来源计入，B1 不检查值何时可得。
`artifact` 在 download 模板检查后、checksum 请求内，以及声明 remote artifact 后可用；
版本请求没有 remote artifact 时不自动提供它。mirror 还可使用 §5.2 的 URL 组件变量，
只对其允许的 registry 作用域检查，空允许列表表示全部。未实例化的 registry / mirror
可保留上下文变量。错误定位到包含引用的实际字段，不定位到渲染结果；不做替换、
URL 校验、环 / 深度 / 大小检查，也不要求未知 parser 可执行，这些由后续卡负责。

### 7.2 B2 模板引擎

- 按 §5.2 区分 `Authored` 和 `Literal`；在作用域内按优先级合并：registry `variables` < resource `source` < variant `source` < artifact `source` < release `source` < 宿主 `source` < 内置变量。download 的 artifact 名渲染后才合并 registry variables；除 §11.1 列出的有意偏差外，与参考实现 `R/resource-template.ts:1068-1110,1336-1346` 对齐，B4 的差分测试负责兜底。
- 用 DFS 检测环，限制深度和输出长度；计数、字段、错误定位见 §5.6 B2 表。
- B2 API：`ValidatedManifest::artifact(resource_id, variant_id, platform)` 只查找明确指定的
  产物，返回借用句柄 `Option<SelectedArtifact>`；不推导默认或 tags，未知组合返回 None。
  `SelectedArtifact::render(ArtifactRenderOptions { version, source }, &Limits)` 返回
  `RenderedArtifact` 或 `RenderError`；结果字段私有，提供只读 `artifact()` 和 `canonical_url()` 访问器。static 未给 version 时取
  latest，指定版本必须在历史中；remote 必须提供已解析版本，匹配 fallback 时合并其 source。
  宿主 source 优先于 release source，与参考实现的 release/source 顺序差异遵循本节优先级。
- `render_template(template, &BTreeMap<&str, TemplateValue>, path, &Limits)` 暴露通用
  渲染原语；`TemplateValue::{Authored, Literal}` 借用字符串，错误提供 `code/path/fields`。
  B1 与 B2 共用 scope 合并、registry variables 层与占位符扫描，B1 不新增渲染校验。
- artifact 渲染后以 Literal 进入 URL 作用域，避免结果被重复展开；renderer 不做百分号编码，
  `canonical_url` 为 `url` 的 WHATWG 规范化结果（如空格/花括号可能由 URL 解析器编码）。
  B3 再增加 version/checksum 请求、mirror 和策略计划，B2 不导出新 wire / TS 类型。
- `url` 2.5 为纯 Rust；B2 用 `wasm32-unknown-unknown` 编译检查其与 core 的兼容性，
  依赖树不得出现 HTTP 客户端或异步运行时。

### 7.3 B3 选择、计划与校验策略

B3 API 均从 `manifest_core` 根导出，输入使用 B2 的 `SelectedArtifact` 借用句柄。
计划及解析结果的字段私有，只经计划函数构造，提供同名只读访问器；可序列化但不反序列化，
防止宿主伪造 kind 或请求与 parser 的绑定。`ChecksumKind`、计划、结果、策略与新增错误
均 derive `specta::Type`；编译后的 parser 和 Limits 属于 native 执行状态，不参与 wire 导出。
`ResolveError` 的 native source 不参与类型导出，宿主按 §5.6 的 code/path/fields 投影错误。

| API                                                                           | 返回值                                           |
| ----------------------------------------------------------------------------- | ------------------------------------------------ |
| `artifact_checksum_kind(&ResourceArtifactTemplate)`                           | `ChecksumKind`                                   |
| `list_variant_checksum_kinds(&ValidatedManifest, resource_id, platform)`      | `Result<Vec<VariantChecksumKind>, ResolveError>` |
| `select_variant(&ValidatedManifest, resource_id, platform, VariantSelection)` | `Result<SelectedArtifact, ResolveError>`         |
| `plan_version(&SelectedArtifact, VersionOptions, &Limits)`                    | `Result<VersionPlan, ResolveError>`              |
| `plan_download(&SelectedArtifact, DownloadOptions, &Limits)`                  | `Result<DownloadPlan, ResolveError>`             |
| `finish_checksum(&ChecksumPlan, &RemoteResponse)`                             | `Result<ResolvedChecksum, ParseError>`           |
| `check_policy(ChecksumKind, &VerifyPolicy)`                                   | `Result<(), VerifyError>`                        |

`VariantSelection { id, tags }` 同时给出两个条件时必须选中同一 variant。
`VersionOptions { source, mirror }` 配置版本请求；
`DownloadOptions { version, source, mirror, checksum_mode }` 配置下载计划。
kind 列表按 manifest 中 variant 顺序，仅列出支持目标平台的 `{ variant_id, kind }`。
`finish_checksum` 使用计划绑定的 parser、原下载产物名与 Limits，结果 kind 始终为 parsed。
`check_policy` 成功即接受，失败通过 verify.kind_rejected 和 `{ kind }` 解释。

- 选择先过滤支持平台的 variant，再按 ID / 精确 tags / 平台默认值选择；tags 空或重复都拒绝，
  不接受子集匹配。宿主 tags 按精确 tag ID 集合比较，不按拼接后的 key 匹配（V-14）。
  未配置默认值且仅有一个候选时直接选择它，与 B1 的有效 manifest 规则一致（V-13）。
- `VersionPlan` 有 `kind/latest/releases/remote` 访问器；remote 的 `request()` 返回
  `RequestPlan`，`parser()` 返回 `CompiledVersionParser`，宿主调用其 `parse` 解析版本。
  `DownloadPlan` 有 `artifact/version/request/kind/checksum/checksum_request` 访问器。
  `RequestPlan` 始终携带 `canonical_url`、可选 `mirror_url`、`kind`；static version 计划没有请求。
- static 下载 kind 取实际选择的 release；static 产物没有 fallback snapshot，
  `checksum_mode` 对其无效，`ChecksumMode::Snapshot` 仍返回正常下载计划。
  remote 默认 `ChecksumMode::Remote`，有规则为
  `parsed`，无规则为 `none`，即使版本匹配 fallback 也不自动降级。显式 `ChecksumMode::Snapshot`
  必须给出恰好等于 fallback.version 的版本且存在 fallback checksum，才输出 `snapshot`，
  此时不生成远端 checksum 请求。未知 version / checksum parser 在所有模式下都先被拒绝。
- mirror 限制与 `R/resource-template.ts:1037-1045` 完全相同：缺省或空 `registries` 接受全部，
  非空必须包含当前请求的 registry。download 用 resource.registry，version / checksum 请求各用
  声明的 remote.resource.registry；不只检查二进制下载。B1 的 canonical() 与计划共用此判断。
  URL 组件以 Literal 进入 mirror 作用域，`url_encoded` 使用 JS encodeURIComponent 的字节规则；
  mirror URL 与 canonical URL 都执行 B2 的 http(s)、userinfo、fragment 检查，mirror 不改变 kind。
- remote source 覆盖一般 source，内置变量最后恢复最高优先级（V-11）；version 请求不合并
  fallback source，checksum 请求基于 B2 已选择 release 与宿主作用域，远端虚拟 artifact 不改变
  GNU / BSD / GitHub parser 要匹配的真实下载产物名。
- `VerifyPolicy { allow_parsed, allow_snapshot }` 实现 `Default`，默认全为 `false`；
  authoritative 始终通过，两个开关分别只接受 parsed / snapshot，none 永不通过。
  宿主同时暴露 kind 与其有效策略决定。digest 计算与比对由宿主完成；不一致时构造
  `VerifyError::CompareDigest { algorithm, expected, actual }`，expected / actual 为不含算法前缀的 hex。

### 7.4 B4 参考差分

- 用一次性脚本（不入库，命令写进测试集 README）对快照中的每个 resource × platform × 默认 variant 运行参考实现，产出 `conformance/resolve/reference-golden.json` 并入库。remote 产物用 `fallback.version` 作为显式版本。
- 测试断言 G5；不一致项要么修 core，要么把差异记进 §11.1。

---

## 8. Phase C：wasm 与构建期胶水脚本

### 8.1 C1 `manifest-wasm`

- `crate-type = ["cdylib"]`；导出 `validateManifest`、`artifactChecksumKind`、`selectVariant`、`planVersion`、`planDownload`、`parseVersion`、`finishChecksum`、`checkPolicy`。
- 值的转换用 `serde-wasm-bindgen`，采用 `Serializer::json_compatible().serialize_maps_as_objects(false)`，
  再递归转换成普通 JS 对象，以 `Object.defineProperty` 写入自有属性。
  任意键 map（source、resources、registries、mirrors 等）的 `__proto__` 键作为数据保留，不调用原型 setter。
  错误一律转成 `{ code, path, message, fields }`。`ValidatedManifest` 以不透明句柄留在 wasm 内。
- C1 的 JS 边界签名如下；`?` 表示可以省略、传 `undefined` 或 `null`，使用默认值。

  | JS API                                                                                                                                                                              | 返回值                                          |
  | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
  | `validateManifest(raw: ResourceManifest)`                                                                                                                                           | `ValidatedManifest` 句柄                        |
  | `artifactChecksumKind(artifact: ResourceArtifactTemplate)`                                                                                                                          | `ChecksumKind` 字符串                           |
  | `selectVariant(manifest: ValidatedManifest, resource: string, platform: SupportedPlatform, selection?: { id?: string, tags?: string[] })`                                           | `SelectedArtifact` 句柄                         |
  | `planVersion(selected: SelectedArtifact, options?: { source?: SourceVariables, mirror?: string }, limits?: LimitsInput)`                                                            | `VersionPlan` 句柄                              |
  | `planDownload(selected: SelectedArtifact, options?: { version?: string, source?: SourceVariables, mirror?: string, checksum_mode?: "remote" \| "snapshot" }, limits?: LimitsInput)` | `DownloadPlan` 句柄                             |
  | `parseVersion(parser: RemoteVersionParser, response: RemoteResponseInput, limits?: LimitsInput)`                                                                                    | 经过 VersionLabel 校验的字符串                  |
  | `finishChecksum(plan: DownloadPlan, response: RemoteResponseInput)`                                                                                                                 | `{ checksum, kind: "parsed" }`                  |
  | `checkPolicy(kind: ChecksumKind, policy?: VerifyPolicy)`                                                                                                                            | `void`（拒绝时抛出）                            |
  | `parseChecksum(parser: RemoteChecksumParser, response: RemoteResponseInput, artifact: string, limits?: LimitsInput)`                                                                | `ResourceChecksum` 字符串                       |
  | `listVariantChecksumKinds(manifest: ValidatedManifest, resource: string, platform: SupportedPlatform)`                                                                              | `VariantChecksumKind[]`                         |
  | `renderTemplate(template: string, values: Record<string, TemplateValueInput>, path: string, limits?: LimitsInput)`                                                                  | 渲染后的字符串                                  |
  | `renderArtifact(selected: SelectedArtifact, options?: { version?: string, source?: SourceVariables }, limits?: LimitsInput)`                                                        | `{ artifact, canonical_url }`                   |
  | `compareDigest(algorithm: ChecksumAlgorithm, expected: string, actual: string)`                                                                                                     | `void`（不一致时抛出 `verify.digest_mismatch`） |

  后五个导出由 C3 为一致性运行器追加（独立 checksum parser、variant kind 列表、模板渲染与 digest 比较）。`TemplateValueInput` 为 `{ authored: string }` 或 `{ literal: string }`。

- `ValidatedManifest.newerMinor` 是只读布尔属性。`SelectedArtifact.value` 返回所选 variant；
  `VersionPlan.value`、`DownloadPlan.value` 返回 core 的 serde 计划投影，wire 字段保持 snake_case。
  每次读取 `value` 都生成新的普通 JS 对象，修改它不改变 wasm 状态。
  Node facade 对所有句柄的 `value` / `newerMinor` getter 和方法使用相同的错误翻译，
  将结构化 wasm 错误转为 `ManifestRuntimeError`；调用 wasm 时解包成原始句柄。
  这些类均无公开构造函数，使用 wasm-bindgen 的 `free()` / `Symbol.dispose` 释放。
  selection 内部共享不可变 manifest 的所有权，因此释放原 manifest 句柄后仍可规划；
  计划拥有执行所需状态，释放 selection 后仍可完成 checksum。
  计划只序列化，不增加 `Deserialize`，`finishChecksum` 只接受真实下载计划句柄；
  从中取得绑定的 ChecksumPlan，不接受 JS 伪造的 parser、artifact 或 kind。
- `RemoteResponseInput` 是 `{ body: string, headers?: [string, string][] }`，headers 默认空数组，
  每项可为单行字段值或客户端合并的逗号列表，允许保留重复项与顺序；不要求宿主取得每次出现的原始头行，header parser 按 §5.3 拆分。`LimitsInput` 的可选字段为 `body_bytes`、`header_value_bytes`、
  `regex_pattern_bytes`、`regex_size_bytes`、`template_depth`、`render_output_bytes`；
  逐字段调用 `Limits::with_*`，只能调低默认上限。core 的 selection/options 与 parser 的
  response 是借用类型，边界先解码其各字段，再构造借用输入，不复制定义对应的 wire struct，
  不给带私有上限不变量的 Limits 添加 `Deserialize`。
  `ChecksumMode` 的 serde / specta 定义直接放在 core，wire 值为 `remote` / `snapshot`。
- JS 输入先生成只含自有可枚举字段的快照，再逐字段直接解码成目标类型，避免先解码为通用 JSON Value。
  快照最多允许沿任一路径嵌套 64 层容器（对象或数组），超过上限及循环引用均以
  `wasm.invalid_input` 拒绝，path 指向触发上限的字段；未知、将被忽略的字段也受此限制。
  `serde-wasm-bindgen` 的 `IgnoredAny` 本身不递归，但 tagged enum 的 serde 缓冲和 parser 的
  自定义反序列化仍可能递归读取通用值，因此 raw manifest、artifact、parser 和其余入口均需快照保护。
  快照同时拒绝任意位置的 NaN / ±Infinity，保留精确参数字段路径，不能将其转换为 null 或缺省值。
  显式 null / undefined 的既有可选参数语义不变；有限 priority 在 ±2^53 边界及之外仍可往返，
  只有绝对值不超过 `Number.MAX_SAFE_INTEGER` 的整数使用 serde 整数序列化，其余使用 f64。
- `parseVersion` 可直接接收规则以覆盖 C3 的独立 parser 语料（包括编译失败用例）；
  解析版本请求时从 `versionPlan.value.remote.parser` 取得规则，并传入规划时相同的 Limits。
  它只返回已验证版本字符串，不接收或生成校验可信度。
- 错误为普通对象（不是 JS Error），`message` 只用于展示，不保证文本稳定。
  `validateManifest` 的语义错误列表统一抛出
  `{ code: "validate.failed", path: "arg.manifest", message, fields: { errors: [{ code, path, message, fields }, ...] } }`，
  保留 core 的错误顺序与每条原始 code/path/fields。
  manifest 版本先按元类型校验，保留 `manifest.invalid_version_format` /
  `manifest.unsupported_major`、`path: "manifest_version"` 和 `{ major }` 字段。
  其余反序列化失败使用 `wasm.invalid_input`、`fields: {}`，path 为对应参数或字段
  （如 `arg.manifest`、`arg.platform`、`arg.options.checksum_mode`、`response.body`）。
  不含远端 checksum 请求的下载计划传给 `finishChecksum` 时，抛出
  `wasm.checksum_plan_missing`、`path: "arg.plan"`、`fields: {}`。
  输出序列化失败使用 `wasm.serialization_failed`、`path: "arg.result"`、`fields: {}`。
  core/parser 执行错误保留 §5.6 的 code/path/fields。句柄必须属于当前 wasm 实例且尚未释放。
- 构建方式：`pnpm build:wasm` 依次执行 `cargo build -p manifest-wasm --target wasm32-unknown-unknown --release` 和 `wasm-bindgen --target web --out-dir packages/manifest-runtime/wasm`。crate 与 CLI 的版本必须一致，并且锁定。
- 构建脚本先比较 CLI 与 Cargo.lock 中的 wasm-bindgen 版本，再执行构建；cargo 使用 `--locked`。
  通过 `cargo metadata` 定位 target_directory，遵守 `CARGO_TARGET_DIR`，产物目录相对仓库根定位。
  release 使用 `opt-level = "s"`、LTO、单 codegen unit 与 strip；LTO 不支持 package override，
  因此这些选项统一放在 workspace 的 `[profile.release]`，普通 test / clippy 仍使用 dev/test profile。
- 产物不入库；CI 中的顺序是 `check:generated` → `build:wasm` → `test`。
- 记录 `.wasm` 原始大小与 gzip 大小，以及 Node 下 `initSync` 的耗时（G11、O-8）。

### 8.2 C2 `@manifest/runtime`

- Node 下用 `readFileSync` 读入 `.wasm` 再 `initSync`，同步初始化；导出类型化函数。
- 计划、kind、错误等类型由 specta 从 core 导出，写进 `packages/manifest-runtime/src/types.ts`（生成物），纳入 `check:generated`。
- 只服务 Node 构建脚本（§5.7）。

### 8.3 C3 Node 一致性测试

`node --test` 读取同一份 `conformance/` 用例，经 `@manifest/runtime` 执行，用例名清单与 Rust 端一致（G3）。

### 8.4 C4 构建期下载校验胶水脚本（`@manifest/downloader`）

- 命令行用法：`manifest-download --manifest <path|url> --resource <id> --platform <p> [--variant <id> | --tags <a,b>] [--mirror <id>] [--allow-parsed] [--allow-snapshot] --out <dir>`。
- 流程：获取 manifest → `validateManifest` → 选择 → 打印 kind 与宿主 `VerifyPolicy` 下的 `checkPolicy` 决定（接受或拒绝及原因）→ 按计划用 `globalThis.fetch` 请求 → `parseVersion` / `finishChecksum` → 下载到临时文件 → 用 `node:crypto` 计算 digest 并比对 → 原子地移入 `--out`。
- 默认只接受 `authoritative`；遇到 `parsed` 时报 `verify.kind_rejected`，提示可以加 `--allow-parsed`。
- HTTP 侧的上限（重定向次数、超时、body 大小）与 §5.5 一致；不读取任何未声明的响应头作为 checksum。
- 所有请求仅接受无 userinfo、无 fragment 的 HTTP(S) URL；重定向的 HTTPS → HTTP 降级在发出下一跳之前报 `http.insecure_redirect`，覆盖 manifest、版本、checksum 和产物。
- HTTP 错误的 `path` 使用稳定定位符 `http.manifest` / `http.version` / `http.checksum` / `http.artifact`；消息中的 URL 去掉 userinfo 和 fragment，查询参数替换为 `?<redacted>`，保留 scheme、host 和 path。
- C4 宿主决定：所有 HTTP 请求集中在 downloader 的 `http.ts`，使用 `globalThis.fetch` 和 `redirect: "manual"`。仅取 parser 声明的头，名称转为小写，存在时把 `headers.get(name)` 的合并值作为单项传给 core；core 按 §5.3 进行逗号列表拆分，宿主不需要取得原始头行。手动跟随最多 5 次重定向；每跳使用一个 `AbortSignal.timeout` 截止时间覆盖响应头和 body 读取，默认 30 秒。未消费的 body 会取消。HTTP manifest 与 parser body 按流限制为 2 MiB，声明的单个收到的 header 值（包括合并值）限制为 8 KiB，在拆分前检查。runtime 尚未导出 Limits 默认值读取接口，宿主按 §5.5 设置这两个上限。
- C4 代理决定：直接依赖 Node 的 fetch 内建环境代理支持，不自行检测代理开关或管理 Agent / socket。依据 [Node.js CLI 文档](https://nodejs.org/api/cli.html#node_use_env_proxy1)，`NODE_USE_ENV_PROXY=1` 从 24.0.0（以及 22.21.0）起适用于 fetch，本项目最低版本 24.2 已支持；[`--use-env-proxy`](https://nodejs.org/api/cli.html#--use-env-proxy) 命令行 / `NODE_OPTIONS` 开关从 24.5.0（以及 22.21.0）起提供。[HTTP 内建代理文档](https://nodejs.org/api/http.html#built-in-proxy-support) 的 24.5.0 门槛对应 `node:http` / `node:https` 的 Agent 支持。进程启动时启用后，由 Node 读取 `HTTP_PROXY` / `HTTPS_PROXY` 与 `NO_PROXY`，未启用时保持 Node 默认行为；代理路由与绕过的子进程测试按各自支持版本执行。
- C4 产物大小决定（本节原先未指定）：流式写盘并计算 digest，默认上限为 1 GiB（1,073,741,824 字节）；通过 `--max-artifact-bytes <bytes>` 配置正的安全整数。此限制独立于 parser body 上限。临时文件放在 `--out` 目录内，校验成功后按产物路径最后一段命名并原子 rename；失败清理临时文件。
- C4 文件名决定：`planDownload` 生成计划后，在请求 checksum 或产物前检查产物路径的最后一段；所有平台统一拒绝 Windows 保留设备名（含扩展名）、非法字符与控制字符、尾随点或空格，以及空、`.`、`..`，报 `download.invalid_filename`。
- C4 snapshot 决定：仅设置 `--allow-snapshot` 时，在请求远端版本前打印 `version=unresolved kind=snapshot` 的预期策略决定；解析版本后使用 core 的 `checksum_mode: "snapshot"`，由 core 检查其与 fallback 版本精确匹配及 checksum 是否存在，确认后再次打印已解析版本与有效策略。两个允许开关同时设置时使用远端 parsed 路径，不自动回退。所有路径在请求版本、checksum 或产物前必须已有策略行；默认拒绝远端产物时版本显示为 `unresolved`，在请求远端元数据前输出策略拒绝原因。

---

## 9. Phase D：编译器与快照（里程碑粒度）

| 里程碑                         | 内容                                                                                                                          | 前置   | 完成判据                                                                                                                                                           |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- | ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| D0 移除 Clash Premium          | 见 §9.3                                                                                                                       | —      | 快照、golden、一致性用例、可执行测试与生成的 manifest 中不再有 `clash_premium`；文档中的历史记录可保留                                                             |
| D1 快照模型                    | `ReleaseSnapshot`：release 与资产 ID、digest、观测时间、来源 URL                                                              | B 完成 | Schema 生成；generator 写出快照文件                                                                                                                                |
| D2 编译器移植                  | `compile(template, snapshot, now) -> ResourceManifest`，纯函数；按码点排序（V-4）；编译期检查模板可解析；写出 `CURRENT_MINOR` | D1     | 同一组输入两次编译的输出逐字节相同；`https://{missing}/{artifact}` 被拒                                                                                            |
| D3 generator 接入              | Node 负责抓取，写快照，再经 wasm 调用 `compile`                                                                               | D2、C2 | 保留的 resource 与上游生成器的输出等价（`serde_json::Value` 相等）；有意的差异只有 `manifest_version`、`clash_premium` 不再生成（§9.3）与 Clash Rust alpha（§9.2） |
| D4 Clash Rust alpha 的版本来源 | 多响应组合版本（§9.2，O-4）：parser、core、wasm、一致性用例与生成物                                                           | C 完成 | 组合版本的正反用例在 Rust 与 Node 两端通过；Clash Rust alpha 为 remote，版本由 `commits/latest` 与 `releases/latest` 组合，checksum 为 `parsed`                    |
| D5 签名                        | minisign；`manifest-signature` 的 `sign` / `verify_sign` 接口（§9.1，O-2）                                                    | D3     | generator 输出的 manifest 带 `.minisig`；Rust 与 Node 宿主跑同一组用例，篡改字节、非可信公钥、损坏签名各有反例                                                     |
| D6 退役上游脚本                | 在 clash-nyanpasu 侧跟踪                                                                                                      | D3、D4 | —                                                                                                                                                                  |

### 9.1 D5 签名接口（O-2）

1. **方案：** 目前只接 minisign。签名对象是发布的 manifest 文件的原始字节；detached 签名与 manifest 同目录发布，命名为 `<manifest 文件名>.minisig`。签名只覆盖 manifest 本身（§5.1 第 1 条）。
2. **封装：** D5 新增叶子 crate `manifest-signature`：无 IO、无 async，不依赖其他 workspace crate，`manifest-core` 也不依赖它。上层只感知签名算法，并提供必要的 key：

   ```rust
   #[non_exhaustive]
   pub enum SignatureAlgorithm {
       Minisign, // wire: "minisign"
   }

   impl PublicKey {
       pub fn parse(algorithm: SignatureAlgorithm, text: &str) -> Result<Self, SignatureError>;
   }
   impl SecretKey {
       pub fn parse(
           algorithm: SignatureAlgorithm,
           text: &str,
           passphrase: Option<&str>,
       ) -> Result<Self, SignatureError>;
   }
   impl Signature {
       pub fn parse(algorithm: SignatureAlgorithm, text: &str) -> Result<Self, SignatureError>;
       pub fn to_text(&self) -> String;
   }

   pub fn sign(key: &SecretKey, message: &[u8]) -> Result<Signature, SignatureError>;
   pub fn verify_sign(
       trusted: &[PublicKey],
       message: &[u8],
       signature: &Signature,
   ) -> Result<(), SignatureError>;
   ```

   - `PublicKey`、`SecretKey`、`Signature` 的字段私有，各自记住所属算法。key 与签名以该算法的标准文本格式传入：minisign 的公钥字符串或 `.pub` 内容、`.key` 内容、`.minisig` 内容。
   - 具体实现的类型、key ID、trusted comment 等细节不出现在公共接口中。替换实现只改本 crate 内部；新增算法只加 `SignatureAlgorithm` 变体，调用方代码不变。
   - 算法不一致、key 或签名格式错误、签名 key 不在 `trusted` 中、验证失败分别报稳定错误码（`signature.*`，D5 卡确定完整列表），用 snafu 定义。

3. **实现选择：** 候选为 jedisct1 的 `minisign`（签名与验证）和 `minisign-verify`（只验证）；2026-11-08 在 crates.io 上分别为 0.10.0 与 0.3.0。D5 卡按 wasm 体积增量（计入 O-8 预算）与能否满足上述封装来选定，只在 `manifest-signature` 内部引用。
4. **使用位置：**
   - 签名在 generator（D3）的 CI 中进行。secret key 与口令由宿主从 CI secret 读取后传入，crate 不读文件或环境变量。
   - 验证在解码 manifest 之前，对原始字节进行。可信公钥由宿主内置，不从 manifest 或网络获取；允许多把以便轮换：只要 `trusted` 中有一把同算法的 key 能通过密码学验证即成功，与顺序无关；`trusted` 为空、缺少签名或验证失败都拒绝整份 manifest，不回退到未签名。D5 卡为新旧 key 重叠期与已撤下的 key 各写用例。
   - Rust 宿主直接依赖 crate；Node 宿主通过 `manifest-wasm` 与 `@manifest/runtime` 调用 `sign` / `verifySign`。downloader 的命令行开关在 D5 卡中确定。
5. **限制：** D5 完成前，`authoritative` 只表示"内嵌在 manifest 中"，宿主不验证签名。签名不防回滚，即重放更早的已签名 manifest；需要时另行设计。

### 9.2 D4 Clash Rust alpha 的版本来源（O-4）

2026-11-08 核对结果：

- 上游已迁到 `ibigbug/clash-rs`（clash-nyanpasu 主仓库也用这个位置）。`Watfaq/clash-rs` 经 301 跳转到 `Watfaq-legacy/clash-rs`，后者的最新稳定版停在 v0.10.10，`ibigbug/clash-rs` 已到 v0.10.11。
- 两处的 `latest/version.txt` 都返回 404；`latest` release 是 prerelease，25 个资产都带 sha256 digest，没有版本文件。
- clash-nyanpasu 后端（`nyanpasu-updater` 的 `fetch_clash_rs_alpha_version`）用两到三次 GitHub API 请求得出版本（annotated tag 需再请求一次解引用）：`git/ref/tags/latest` 的 commit 前 7 位，加 `releases/latest` 的稳定 tag，拼成 `0.10.11-alpha+sha.bd22442`；它留有 TODO(#1112)，要求 manifest v2 声明滚动版本的取法。现有 version parser 只能从一个响应取一个值，表达不了。

决定：alpha 保持 `remote`，checksum 仍由 `github-release-assets` parser 从 `expanded_assets/latest` 解析（`parsed`）；扩展协议，让 remote version 可以由多个响应组合。

1. **wire：** remote 的 `version` 在现有单响应形式 `{ resource, parser }` 之外，新增组合形式 `{ parts, format }`。两种形式以是否有 `format` 区分，精确的 serde 表示在 D4 卡中确定。

   ```json
   "version": {
     "parts": {
       "stable": {
         "resource": { "registry": "github-api", "artifact": "releases/latest" },
         "parser": { "type": "regex", "pattern": "\"tag_name\":\\s*\"v?([0-9][^\"]*)\"" }
       },
       "commit": {
         "resource": { "registry": "github-api", "artifact": "commits/{release}" },
         "parser": { "type": "regex", "pattern": "\"sha\":\\s*\"([0-9a-f]{7})[0-9a-f]{33}\"" }
       }
     },
     "format": "{stable}-alpha+sha.{commit}"
   }
   ```

   示例中的 `github-api` 是 manifest 新增的 registry，`url_template` 为 `https://api.github.com/repos/{owner}/{repo}/{artifact}`。`commits/{ref}` 端点会把 annotated tag 解引用到 commit。

2. **语义：**
   - 每个 part 是一次普通的 remote version 请求：同样的 parser 语义（§5.3）、同样的单响应上限（§5.5），结果各自满足 `VersionLabel`。
   - `parts` 至少 1 项、至多 4 项；名称满足变量名规则，不能与内置变量同名。
   - `format` 是 Authored 模板，只能引用 part 名称，且必须引用每个 part；part 结果以 Literal 代入，不再展开。组合结果必须满足 `VersionLabel`，否则报 `parser.invalid_version`。
   - 任何一个 part 失败，整个版本失败，不使用部分结果。
3. **计划与宿主：** 版本计划按 part 名称的码点顺序列出每个 part 的请求，宿主全部执行后把各自的响应交给 core 组合。响应与 part 的对应方式、缺失或多余响应的处理，以及错误路径（组合结果没有单一的 `response.body`，不能沿用 §5.6 的 version parser 路径）在 D4 卡中确定。mirror、`ChecksumKind` 与下载计划的规则不变。
4. **校验与兼容：**
   - `validate_manifest` 检查 part 数量、名称、`format` 的引用完整性与 registry 引用；错误码在 D4 卡中按 §5.6 的规则确定。
   - 这是 2.0 内的契约新增（O-11），生成的 TS、Schema 与 runtime 类型出现新形式，属于预期变化。
   - 宽松解码与单响应形式一致：无法识别的 part parser 使该产物 `resolve.unsupported_*`（G8）。
5. **Clash Rust alpha 的 manifest：**
   - `source.owner` 为 `ibigbug`，不依赖 GitHub 的跳转：旧路径已经指向落后的仓库。
   - generator 写出的 `fallback.version` 使用同一格式，使显式 snapshot 策略的精确匹配可用。
6. **已知代价：**
   - 每次解析需要 2 次未认证的 GitHub API 请求，同一 IP 每小时限 60 次。clash-nyanpasu 目前的做法也是这样。
   - version 与 checksum 分别请求。上游在两次请求之间重新发布时，版本号可能落后一个构建，但下载的字节仍按 `expanded_assets` 的 digest 校验。

### 9.3 移除 Clash Premium

- manifest 不再包含 `clash_premium`，generator（D3）不生成它。
- D0 卡删除快照 `crates/manifest-metadata/tests/fixtures/resources.json`、`conformance/resolve/reference-golden.json`、`conformance/validate/manifest.json` 中的 `clash_premium`，以及以它为测试对象的用例（`crates/manifest-metadata/src/lib.rs`、`packages/manifest-metadata/main_test.ts`）；仍需同类覆盖的用例改用其他 resource。G5 的 golden 随之变化。
- `D/` 的示例已在本次修订中删除。`files/` 是只读的上游参考，不改。

---

## 10. 迁移不变量

1. 在 `manifest_version` 改为字符串（S0f）之前，快照反序列化再序列化后与原快照在 `serde_json::Value` 层面相等。S0f 同一提交把快照的 `manifest_version` 改为 `"2.0"`，此后照常比较。
2. 生成的 TS 与 Schema 只允许 G7 列出的变化。
3. 搬迁卡只搬代码；行为变化（§11.1 的每一条）各自单独成卡、单独提交，并附带测试集用例。
4. 快照中的合法输入，resolver 输出的 URL 与参考实现逐字节相同（G5）。
5. 每个提交都能编译、测试通过，且只做一件事；调用方在同一提交里迁完，不留 re-export、别名或兼容层。
6. 发布前版本号固定为 `"2.0"`（O-11）。

---

## 11. 与现设计的偏差和决策

### 11.1 偏差（S0a 卡同步修订 `D/`）

| #    | 现设计 / 参考实现                                                                    | 本设计                                                                                                                          | 理由                                                                                        |
| ---- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| V-1  | 结果中分开给出 `checksum`、`fallback_checksum`、`checksum_remote`；mirror 不区分信任 | 统一用 `ChecksumKind`（`authoritative`/`parsed`/`snapshot`/`none`），可静态推出；默认只接受 `authoritative`；mirror 不影响 kind | O-1：只签名权威来源；前端需要提前知道 kind                                                  |
| V-2  | 模板变量值递归展开；远端 version 不限制字符集                                        | 区分 `Authored`/`Literal`；`VersionLabel`；最终 URL 不带 userinfo 和 fragment                                                   | §1.2 已复现的注入                                                                           |
| V-3  | `github-release-assets` 在 2048 字符窗口内按邻近查找                                 | 按条目结构解析，名称完全相等                                                                                                    | §1.3 已复现两种错误匹配                                                                     |
| V-4  | 排序用 `localeCompare`                                                               | 按 Unicode 码点排序                                                                                                             | 跨运行时的确定性；快照中所有 key 与 ID 是 ASCII 小写，两种排序结果相同（2026-11-06 已核对） |
| V-5  | `trim: false` 无效；BSD 不核对标签；regex 是 JS 方言                                 | 按 §5.3 实现                                                                                                                    | 两种语言实现后不能出现偏差                                                                  |
| V-6  | 审查中有人建议 wasm 边界用 tsify                                                     | 只用 specta；wasm 边界用 `serde-wasm-bindgen`                                                                                   | 同一批领域类型只能有一个导出器                                                              |
| V-7  | 声称编译期拒绝无法解析的变量，实际没检查（`D/:333`）                                 | B1 检查依赖是否完整；D2 在编译时检查                                                                                            | §1.4                                                                                        |
| V-8  | "Rust 消费者实现相同语义"（`D/:214-216`）                                            | "Rust 消费者依赖 `manifest-core`"                                                                                               | 消除第二份实现                                                                              |
| V-9  | `manifest_version: 2`（整数）                                                        | `manifest_version: "2.0"`（`major.minor`），消费侧对新 minor 宽松                                                               | O-7                                                                                         |
| V-10 | parser 只读 body                                                                     | 输入是 `RemoteResponse`（body 加 headers）；新增 `header` checksum parser；禁止隐式从响应头取 checksum                          | O-1                                                                                         |
| V-11 | remote source 可覆盖内置变量                                                         | 内置变量始终拥有最高优先级                                                                                                      | 防止 manifest 覆盖 version、platform 与 resource / variant 身份                             |
| V-12 | download 的 release source 覆盖宿主 source                                           | 宿主 source 优先于 release source                                                                                               | §7.2 的显式合并顺序；宿主值仍必须先通过 VersionLabel                                        |
| V-13 | 缺少平台 default_variants 时，即使唯一候选也失败                                     | 唯一候选可直接选中；多个候选仍必须配置默认                                                                                      | B1 已允许单候选省略默认，选择层保持同一有效性规则                                           |
| V-14 | 宿主 tags 拼接后比较 `tag_key`，`["a+b"]` 可选中 `[a, b]`                            | 按 tag ID 集合精确比较；合法 tag ID 结果相同                                                                                    | 防拼接歧义（§7.1 tag ID 不含 `+`）                                                          |
| V-15 | `RemoteResponse.headers` 每项对应原始头行                                            | 每项可为单行或逗号合并值；header parser 按 §5.3 拆分、去 OWS、忽略空元素后判断                                                  | 标准 fetch API 只暴露合并值，契约须支持标准客户端                                           |

### 11.2 决策

A2 已用真实响应复核 §5.5 上限并保持不变：mihomo expanded_assets 为 523,715 B，
checksums.txt 为 13,835 B，最大 header 值为 3,969 B。

| #    | 问题                             | 结论                                                                                                                                                                                                                                                  |
| ---- | -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| O-1  | 默认校验策略                     | **已定（2026-11-06）：** 只接受权威来源（manifest 内嵌、随签名覆盖）的 checksum。经 parser 取得的一律是 `parsed`；从 header 取必须由 parser 规则显式声明；前端感知 kind 并提示（§5.1）                                                                |
| O-2  | manifest 签名                    | **已定（2026-11-08）：** 签名只覆盖 manifest 本身；方案只接 minisign；上层只通过 `sign` / `verify_sign` 接口感知算法并提供 key（§9.1，D5）                                                                                                            |
| O-3  | `github-release-assets` 的解析器 | **已定（A4，2026-11-06）：** 用轻量 tokenizer `tl`；fixture 结果与完整 DOM 相同，wasm 增量约 64 KiB 对 605 KiB（测量见 `conformance/fixtures/github-expanded-assets/README.md`）                                                                      |
| O-4  | Clash Rust alpha 的版本来源      | **已定（2026-11-08）：** 上游没有 `version.txt`；扩展协议，remote version 可由多个响应各自解析后按模板组合（§9.2，D4）；alpha 保持 remote，checksum 由 parser 解析为 `parsed`；owner 为 `ibigbug`                                                     |
| O-5  | 模板侧是否 `deny_unknown_fields` | **已定（2026-11-08）：** 是；S0c 已为模板侧专用类型开启，编译产物侧保持宽松（§5.4 第 5 条）                                                                                                                                                           |
| O-6  | wasm 的消费者                    | **已定（2026-11-06）：** 只有 Node 包，用于构建期胶水脚本下载并校验资源                                                                                                                                                                               |
| O-7  | 版本与兼容方式                   | **已定（2026-11-06）：** 参考 Docker 采用 `major.minor`；兼容更新只升 minor（§5.4）                                                                                                                                                                   |
| O-8  | wasm 体积预算                    | **已定（2026-11-08）：** 接受 C1 实测体积：raw 1,513,596 B，gzip（level 9）535,484 B；Node initSync 五次中位数 1.963 ms。预算采用 C1 建议值：gzip ≤ 803,226 B（当时的 1.5 倍），初始化中位数 ≤ 10 ms；测量环境与方法见下文                            |
| O-9  | 本仓库的 Rust 规则               | **已定（2026-11-08）：** 采用 `docs/development/rust.md`（改编自 clash-nyanpasu）                                                                                                                                                                     |
| O-10 | crate 和包的正式名称             | **已定（2026-11-08）：** 现有名称即正式名称：crate `manifest-metadata`、`manifest-parser`、`manifest-core`、`manifest-wasm`、`manifest-schema-generator`；包 `@manifest/metadata`、`@manifest/runtime`、`@manifest/downloader`、`@manifest/generator` |
| O-11 | 发布前的版本号                   | **已定（2026-11-06）：** 包首次发布前保持 `"2.0"`，这期间的契约改动都并入 2.0，不升版本。理解是"发布前不升 schema 版本"，并非"发布前完全不改契约"；若理解有误，以维护者更正为准                                                                       |
| O-12 | Schema `$id` 前缀                | **已定（本 PR，2026-11-06）：** `https://libnyanpasu.github.io/resources-manifest/schema/`；后接 `2.0/manifest.schema.json` 或 `2.0/manifest-template.schema.json`                                                                                    |

C1 测量使用 Windows x86_64、Node v26.12.1、wasm-bindgen 0.2.114、Rust stable 1.99.1
和 §8.1 的 release profile。指定的 Rust 1.95.0 在依赖编译阶段 ICE，重跑仍复现，
因此以上是 stable 回退构建的实测值，不视作 1.95 验证通过。
原始大小统计 bindgen 输出的 `manifest_wasm_bg.wasm`；gzip 使用 Node `zlib.gzipSync`
且 level 为 9。每次启动一个新的 Node 进程，在导入 JS glue 后计时，范围包含
`readFileSync` 与 `initSync({ module: bytes })`，不包含进程启动和 glue 导入。
五次耗时依次为 9.978、1.856、4.790、1.768、1.963 ms；预算为 C1 建议值，
后续应在修复后的指定工具链与同一测量环境复核。

---

## 12. 不在范围

- manifest 签名的实现（D5；方案已定为 minisign，见 §9.1）。
- i18n：只在 `D/` 中写一句"UI 以 `tag.id` 和 resource key 作为翻译键，`name` 只作兜底"，不改类型。
- mirror 元数据扩展、平台映射 helper、resource 的 icon 或 homepage。
- `@manifest/*` 的 npm 发布与 JS 构建；浏览器和 bundler 接入 wasm。
- clash-nyanpasu 侧的接入：updater 迁移、IPC、UI 的提示文案与交互。本 spec 只提供 `ChecksumKind` 及其静态推导。
- version parser 从 header 读取版本。
- Phase D 的卡片级细化。
