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
5. **默认只接受权威 checksum（O-1，已定）。** 只有 manifest 内嵌、随 manifest 签名的 checksum 才算权威。远端经 parser 规则取得的 checksum（不论来自 body 还是 header，不论走 canonical 还是 mirror）都标为 `Parsed`，默认被拒。从 header 取 checksum 必须由 parser 规则显式声明，宿主不得自行从响应头推断。每个产物的 `ChecksumKind` 不联网就能从 manifest 推出，前端据此在更新 alpha 这类内核前提示用户（§5.1）。
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

| 事实                                                                                                                                   | 位置                                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 编译阶段只检查注册表模板是否非空、变量名是否合法，从不试渲染；`https://{missing}/{artifact}` 能通过编译                                | `R/resource-template.ts:579-588`、`:808-863`；`D/:333` 声称会拒绝                                                                         |
| 排序用 `localeCompare`，结果依赖运行时的 locale                                                                                        | `R/resource-template.ts:558`、`:800`、`:838`                                                                                              |
| 时间戳在生成时取 `new Date()`，网络抓取和编译没有分开                                                                                  | `R/generate-latest-version.ts:126-128`                                                                                                    |
| Clash Rust alpha：生成端用 `git/ref/tags/latest` 的 commit 加稳定版版本号拼出版本，消费端却去请求 `latest/version.txt`，两种算法不等价 | `R/manifest.ts:327-345`；`R/core-resource-templates.ts:131-135`、`:380-386`；`crates/manifest-metadata/tests/fixtures/resources.json:321` |

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
| G9  | 每个产物的 `ChecksumKind` 不联网就能得出；默认策略只接受 `Authoritative`                                                                                                         | `artifact_checksum_kind` 的用例；胶水脚本对 `Parsed` 产物默认失败的测试                                                                                                      |
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

1. **信任锚点是签过名的 manifest。** 签名只覆盖 manifest 本身，所以只有 manifest 内嵌的 checksum 是权威的（签名方案见 O-2；本 spec 只确定签名的覆盖范围）。
2. **`ChecksumKind` 有四种：**

   | kind            | 来源                                                                                                 | 默认策略                           |
   | --------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------- |
   | `Authoritative` | static 产物的 release 中内嵌的 `checksum`                                                            | 接受                               |
   | `Parsed`        | remote 产物按 parser 规则从远端响应解析出的 checksum（body 或 header；不论走 canonical 还是 mirror） | 拒绝，需要宿主显式开启             |
   | `Snapshot`      | remote 产物 `fallback` 中内嵌的 checksum，只在解析出的版本恰好等于 `fallback.version` 时适用         | 拒绝，需要宿主显式开启（离线策略） |
   | `None`          | 没有任何 checksum                                                                                    | 一律拒绝，不提供开关               |

   md5 不改变 kind；宿主可以另外按算法拒绝。

3. **kind 静态可推。** `artifact_checksum_kind(&ResourceArtifactTemplate) -> ChecksumKind` 只看 manifest：static 且该 release 有 checksum 时为 `Authoritative`；static 但没有 checksum 时为 `None`；remote 且声明了 `checksum` 规则时为 `Parsed`；remote 且没有规则时为 `None`。前端（经 clash-nyanpasu 后端）在用户选择、更新 alpha 这类内核**之前**就能拿到 kind 并提示。下载计划和最终结果也带同一个 kind，作为执行时的依据。
4. **只有 parser 规则能产生 `Parsed`。** 宿主不得自行从响应头（`Digest`、`Repr-Digest`、`ETag`、`x-*-checksum` 等）或任何未声明的来源推断 checksum。要用 header，必须在 manifest 里写 `header` parser 规则（§5.3）。
5. **mirror 只负责传输**，不影响 kind。下载计划同时给出 `canonical_url` 和可选的 `mirror_url`，宿主把实际使用的通道交回 core，只用于诊断和日志。
6. **宿主策略**（胶水脚本的默认值）：只接受 `Authoritative`。`--allow-parsed`、`--allow-snapshot` 分别放开对应 kind，并输出警告。不论哪种 kind，校验失败都在解压或替换之前终止。

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

parser 的输入是 `RemoteResponse { body: &str, headers: &[(String, String)] }`；header 名称不区分大小写。body 超过上限（§5.5）直接报 `body_too_large`。

| parser                  | 语义                                                                                                                                                                                                                             |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `text`                  | `trim` 默认 `true`：先去掉首尾空白再校验；为 `false` 时 body 原样校验。结果必须满足 `VersionLabel`                                                                                                                               |
| `regex`                 | 方言是 Rust `regex` crate 语法，不带 flag，可以用内联 `(?i)` 等。取第一个匹配的 `group`（默认 1，0 表示整个匹配），去掉首尾空白后必须满足 `VersionLabel`。校验时检查：pattern 能编译、编译大小不超上限、`group` 小于捕获组数量   |
| `checksum`              | body 去掉首尾空白后按 `ResourceChecksum` 解析                                                                                                                                                                                    |
| `gnu`                   | 按行切分（`\n` 或 `\r\n`），行的格式为 `<hex>[ \t]+[* ]?<name>[ \t]*`；名称完全相等；hex 长度必须等于该算法的长度。同名多行、digest 相同时取一条；digest 不同报 `ambiguous`；没有匹配报 `not_found`                              |
| `bsd`                   | 行的格式为 `<LABEL> (<name>) = <hex>`；`LABEL` 不区分大小写，必须等于配置的算法，不等的行跳过；名称完全相等；冲突报 `ambiguous`                                                                                                  |
| `github-release-assets` | 把 HTML 切成资产条目：名称取自下载链接 `href` 的最后一个路径段（百分号解码后），digest 只从同一条目内读取。名称必须完全相等；没有匹配报 `not_found`；匹配条目没有 digest 报 `digest_missing`；同名条目 digest 冲突报 `ambiguous` |
| `header`（新增）        | `{ type: "header", name, algorithm, encoding: "hex" \| "base64" }`：读取指定的响应头。没有这个头报 `not_found`；同名头出现多次且值不同报 `ambiguous`；`base64` 先解码再转成小写 hex；长度必须等于该算法的长度                    |

version parser 不支持 header，需求出现时再按 minor 增加。

### 5.4 版本与兼容策略（参考 Docker）

1. **格式：** `manifest_version` 是字符串 `"<major>.<minor>"`，如 `"2.0"`。
   - TS 类型：`` `2.${number}` ``。
   - Schema：`pattern: "^2\\.(0|[1-9][0-9]*)$"`。
   - Rust：`ManifestVersion { minor: u32 }`，major 是常量 2。
   - 生产者写出当前的 `CURRENT_MINOR`。
2. **消费规则：** major 不等于 2 时拒绝（`manifest.unsupported_major`）。minor 任意，都接受。minor 大于已知值时，结果中带 `newer_minor: true`，供诊断。
3. **只升 minor 的兼容更新**：旧消费者要么能忽略新增内容，要么只对受影响的那个产物报明确的不支持错误，不影响其他产物。
   - 新增可选字段：旧消费者忽略。
   - 新增 platform：旧消费者跳过未知的 platform key。
   - 新增 parser type 或 checksum parser type：旧消费者解码为"不支持"，只在解析该产物时报 `resolve.unsupported_parser`。旧消费者对这类产物推出的 kind 为 `None`，按默认策略被拒。
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

具体数值在 A2 卡用真实页面大小复核，改动记进 §11.2。

### 5.6 错误码

错误是带结构字段的类型化枚举（rust.md §7.2，用 snafu）。每个变体有稳定的 `code()`；跨语言只靠 `code` 加结构字段识别，不解析 `message`。

| 前缀        | 码                                                                                                                                                               |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `manifest.` | `unsupported_major`、`invalid_version_format`                                                                                                                    |
| `parser.`   | `body_too_large`、`not_found`、`ambiguous`、`digest_missing`、`invalid_version`、`invalid_checksum`、`invalid_encoding`、`invalid_pattern`、`group_out_of_range` |
| `template.` | `unknown_variable`、`cycle`、`depth_exceeded`、`output_too_large`、`invalid_literal`                                                                             |
| `url.`      | `invalid`、`unsupported_scheme`、`forbidden_component`                                                                                                           |
| `select.`   | `unknown_resource`、`unsupported_platform`、`unknown_variant`、`no_exact_tags`、`conflicting_selection`、`default_missing`                                       |
| `resolve.`  | `version_required`、`version_not_in_history`、`mirror_not_allowed`、`unknown_registry`、`unknown_mirror`、`unsupported_parser`                                   |
| `verify.`   | `kind_rejected`（带 `kind`）、`digest_mismatch`                                                                                                                  |
| `validate.` | 每条不变量一个码（B1 卡给出完整列表）                                                                                                                            |

错误都带 `path`：`resources.<id>.variants[<i>]...` 这类 JSON 路径，或参数名。

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

### 7.2 B2 模板引擎

- 按 §5.2 区分 `Authored` 和 `Literal`；在作用域内按优先级合并：registry `variables` < resource `source` < variant `source` < artifact `source` < release `source` < 宿主 `source` < 内置变量。这与参考实现 `R/resource-template.ts:1068-1110` 逐项对齐，B4 的差分测试负责兜底。
- 用 DFS 检测环，限制深度和输出长度。

### 7.3 B3 选择、计划与校验策略

- `select_variant`：按 id、精确 tag 或平台默认值选择，语义与 `R/resource-template.ts:887-943` 一致。
- `artifact_checksum_kind(&ResourceArtifactTemplate) -> ChecksumKind`（§5.1 第 3 条），并为 UI 提供"列出某 resource 在某平台下每个 variant 的 kind"的便捷函数。
- `plan_version` 与 `plan_download` 返回请求计划：`canonical_url`、可选的 `mirror_url`、已编译的 parser、`ChecksumKind`。
- `finish_checksum(plan, &RemoteResponse) -> Result<ResolvedChecksum, _>`：返回 checksum 和 `kind: Parsed`。
- `VerifyPolicy { allow_parsed, allow_snapshot }`（实现 `Default`，默认全为 `false`）与 `check_policy(kind, &VerifyPolicy) -> Result<(), VerifyError>`。digest 比对由宿主完成，因为哈希计算属于 IO 侧；比对失败统一报 `verify.digest_mismatch`。
- `ChecksumKind`、计划与错误类型 derive `specta::Type`，经 clash-nyanpasu 的 bindings 暴露给前端。

### 7.4 B4 参考差分

- 用一次性脚本（不入库，命令写进测试集 README）对快照中的每个 resource × platform × 默认 variant 运行参考实现，产出 `conformance/resolve/reference-golden.json` 并入库。remote 产物用 `fallback.version` 作为显式版本。
- 测试断言 G5；不一致项要么修 core，要么把差异记进 §11.1。

---

## 8. Phase C：wasm 与构建期胶水脚本

### 8.1 C1 `manifest-wasm`

- `crate-type = ["cdylib"]`；导出 `validateManifest`、`artifactChecksumKind`、`selectVariant`、`planVersion`、`planDownload`、`parseVersion`、`finishChecksum`、`checkPolicy`。
- 值的转换用 `serde-wasm-bindgen`，采用 `Serializer::json_compatible()`。错误一律转成 `{ code, path, message, fields }`。`ValidatedManifest` 以不透明句柄留在 wasm 内。
- 构建方式：`pnpm build:wasm` 依次执行 `cargo build -p manifest-wasm --target wasm32-unknown-unknown --release` 和 `wasm-bindgen --target web --out-dir packages/manifest-runtime/wasm`。crate 与 CLI 的版本必须一致，并且锁定。
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
- 流程：获取 manifest → `validateManifest` → 选择 → 打印 kind → `checkPolicy` → 按计划用 `fetch` 请求 → `parseVersion` / `finishChecksum` → 下载到临时文件 → 用 `node:crypto` 计算 digest 并比对 → 原子地移入 `--out`。
- 默认只接受 `Authoritative`；遇到 `Parsed` 时报 `verify.kind_rejected`，提示可以加 `--allow-parsed`。
- HTTP 侧的上限（重定向次数、超时、body 大小）与 §5.5 一致；不读取任何未声明的响应头作为 checksum。

---

## 9. Phase D：编译器与快照（里程碑粒度）

| 里程碑                         | 内容                                                                                                                          | 前置   | 完成判据                                                                    |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- | ------ | --------------------------------------------------------------------------- |
| D1 快照模型                    | `ReleaseSnapshot`：release 与资产 ID、digest、观测时间、来源 URL                                                              | B 完成 | Schema 生成；generator 写出快照文件                                         |
| D2 编译器移植                  | `compile(template, snapshot, now) -> ResourceManifest`，纯函数；按码点排序（V-4）；编译期检查模板可解析；写出 `CURRENT_MINOR` | D1     | 同一组输入两次编译的输出逐字节相同；`https://{missing}/{artifact}` 被拒     |
| D3 generator 接入              | Node 负责抓取，写快照，再经 wasm 调用 `compile`                                                                               | D2、C2 | 与上游生成器的输出等价（`serde_json::Value` 相等，`manifest_version` 除外） |
| D4 Clash Rust alpha 的版本来源 | 二选一（O-4）                                                                                                                 | D3     | 生成端与消费端对同一组采集到的响应得到相同版本                              |
| D5 签名                        | 签名方案和签名流程另出 spec（O-2）                                                                                            | D3     | —                                                                           |
| D6 退役上游脚本                | 在 clash-nyanpasu 侧跟踪                                                                                                      | D3、D4 | —                                                                           |

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
| V-1  | 结果中分开给出 `checksum`、`fallback_checksum`、`checksum_remote`；mirror 不区分信任 | 统一用 `ChecksumKind`（`Authoritative`/`Parsed`/`Snapshot`/`None`），可静态推出；默认只接受 `Authoritative`；mirror 不影响 kind | O-1：只签名权威来源；前端需要提前知道 kind                                                  |
| V-2  | 模板变量值递归展开；远端 version 不限制字符集                                        | 区分 `Authored`/`Literal`；`VersionLabel`；最终 URL 不带 userinfo 和 fragment                                                   | §1.2 已复现的注入                                                                           |
| V-3  | `github-release-assets` 在 2048 字符窗口内按邻近查找                                 | 按条目结构解析，名称完全相等                                                                                                    | §1.3 已复现两种错误匹配                                                                     |
| V-4  | 排序用 `localeCompare`                                                               | 按 Unicode 码点排序                                                                                                             | 跨运行时的确定性；快照中所有 key 与 ID 是 ASCII 小写，两种排序结果相同（2026-11-06 已核对） |
| V-5  | `trim: false` 无效；BSD 不核对标签；regex 是 JS 方言                                 | 按 §5.3 实现                                                                                                                    | 两种语言实现后不能出现偏差                                                                  |
| V-6  | 审查中有人建议 wasm 边界用 tsify                                                     | 只用 specta；wasm 边界用 `serde-wasm-bindgen`                                                                                   | 同一批领域类型只能有一个导出器                                                              |
| V-7  | 声称编译期拒绝无法解析的变量，实际没检查（`D/:333`）                                 | B1 检查依赖是否完整；D2 在编译时检查                                                                                            | §1.4                                                                                        |
| V-8  | "Rust 消费者实现相同语义"（`D/:214-216`）                                            | "Rust 消费者依赖 `manifest-core`"                                                                                               | 消除第二份实现                                                                              |
| V-9  | `manifest_version: 2`（整数）                                                        | `manifest_version: "2.0"`（`major.minor`），消费侧对新 minor 宽松                                                               | O-7                                                                                         |
| V-10 | parser 只读 body                                                                     | 输入是 `RemoteResponse`（body 加 headers）；新增 `header` checksum parser；禁止隐式从响应头取 checksum                          | O-1                                                                                         |

### 11.2 决策

| #    | 问题                             | 结论                                                                                                                                                                                   |
| ---- | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| O-1  | 默认校验策略                     | **已定（2026-11-06）：** 只接受权威来源（manifest 内嵌、随签名覆盖）的 checksum。经 parser 取得的一律是 `Parsed`；从 header 取必须由 parser 规则显式声明；前端感知 kind 并提示（§5.1） |
| O-2  | manifest 签名                    | **部分已定（2026-11-06）：** 签名只覆盖权威来源，即 manifest 本身。方案（minisign/ed25519 等）与流程另出 spec（D5）                                                                    |
| O-3  | `github-release-assets` 的解析器 | **待定：** A4 实测后在轻量 tokenizer 与完整 DOM 之间选一个。基于 JSON API 的 parser 不在本 spec 内                                                                                     |
| O-4  | Clash Rust alpha 的版本来源      | **待定：** D4 开工前确认 `latest/version.txt` 的实际内容                                                                                                                               |
| O-5  | 模板侧是否 `deny_unknown_fields` | **推荐：是**（S0c）；编译产物侧保持宽松（§5.4 第 5 条）                                                                                                                                |
| O-6  | wasm 的消费者                    | **已定（2026-11-06）：** 只有 Node 包，用于构建期胶水脚本下载并校验资源                                                                                                                |
| O-7  | 版本与兼容方式                   | **已定（2026-11-06）：** 参考 Docker 采用 `major.minor`；兼容更新只升 minor（§5.4）                                                                                                    |
| O-8  | wasm 体积预算                    | **待定：** C1 实测后定                                                                                                                                                                 |
| O-9  | 本仓库的 Rust 规则               | **推荐：** 采用 clash-nyanpasu `rust.md`                                                                                                                                               |
| O-10 | crate 和包的正式名称             | 沿用工作名，在创建该包的 PR 中确定                                                                                                                                                     |
| O-11 | 发布前的版本号                   | **已定（2026-11-06）：** 包首次发布前保持 `"2.0"`，这期间的契约改动都并入 2.0，不升版本。理解是"发布前不升 schema 版本"，并非"发布前完全不改契约"；若理解有误，以维护者更正为准        |

---

## 12. 不在范围

- manifest 签名的方案与实现（O-2、D5）。
- i18n：只在 `D/` 中写一句"UI 以 `tag.id` 和 resource key 作为翻译键，`name` 只作兜底"，不改类型。
- mirror 元数据扩展、平台映射 helper、resource 的 icon 或 homepage。
- `@manifest/*` 的 npm 发布与 JS 构建；浏览器和 bundler 接入 wasm。
- clash-nyanpasu 侧的接入：updater 迁移、IPC、UI 的提示文案与交互。本 spec 只提供 `ChecksumKind` 及其静态推导。
- version parser 从 header 读取版本。
- Phase D 的卡片级细化。
