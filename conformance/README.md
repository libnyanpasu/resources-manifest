# 一致性测试集

本目录是 S0b 建立的跨语言用例骨架，供后续 native Rust 与 Node（wasm）运行器读取。
期望值以 [本次设计](../docs/spec/2026-11-06-manifest-core-parsers/design.md)
§5 为准，包含尚待实现的修正语义；当前阶段只验证数据格式、名称唯一性及参考正常路径，
不代表 parser 或 core 已实现这些行为。

## 用例格式

每个 JSON 文件都是数组，每项形如：

```json
{
  "name": "version.text.trim_false_newline",
  "input": {
    "parser": { "type": "text", "trim": false },
    "response": { "body": "v1.2.3\n", "headers": [] }
  },
  "expect": { "error": { "code": "parser.invalid_version" } }
}
```

- `name` 在所有目录、所有 JSON 文件中全局唯一，使用 dotted prefix；运行器以完整名称标识用例。
- `input` 总是对象，各目录的形状和操作见下表。
- `expect` 恰有一个分支：`{ "ok": <结果> }` 或 `{ "error": { "code": "...", "path": "..." } }`。
  `path` 可省略；省略表示不约束路径，有值时精确比较。错误只比较稳定的 `code` 和声明的 `path`，不比较消息文本。
- 结果按 JSON 结构相等比较，对象键顺序无关，数组顺序保留。不得先 trim、规范化输入或丢弃重复 header。
- checksum 成功结果是 `ResourceChecksum` 的序列化字符串：`<algorithm>:<lowercase-hex>`。
  SHA-256 digest 使用 64 位 hex，base64 输入表示同样的 32 字节。

| 目录 / 文件              | `input` 形状与运行器操作                                                                                         | `expect.ok`                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `parsers/version.json`   | `{ parser, response: { body, headers } }`；编译 version parser，再解析响应                                       | version 字符串                       |
| `parsers/checksum.json`  | `{ parser, response: { body, headers }, artifact }`；按 artifact 文件名解析 checksum                             | checksum 字符串                      |
| `validate/manifest.json` | 完整编译产物 manifest；先消费侧解码，再调用 `validate_manifest`，不补字段、不从外部 fixture 加载或合并           | `true`，表示校验成功                 |
| `compat/manifest.json`   | 完整编译产物 manifest；以已知版本 2.0 宽松解码，再按下文约定逐产物检查支持性，不补字段                           | 兼容性投影对象                       |
| `kind/artifact.json`     | artifact template wire object：`{ artifact, source?, version }`；调用 `artifact_checksum_kind`，无请求或响应输入 | kind 字符串                          |
| `resolve/`               | 由 B4 填充 `reference-golden.json`；当前仅有 `.gitkeep`，没有 resolver 用例                                      | B4 定义下载计划与 URL 的参考比较结果 |

`response.headers` 是 `[name, value]` 二元组数组，例如
`[["X-Checksum-Sha256", "..."], ["x-checksum-sha256", "..."]]`。
它保留原始大小写、顺序和同名多值，不转换为可能合并值的普通对象。
parser 对象使用 manifest 的 `type` tag 和字段名：version 的 `text` / `regex`，
checksum 的 `checksum` / `gnu` / `bsd` / `github-release-assets`，以及设计新增的 `header`。
`header` 的 `encoding` 为 `hex` 或 `base64`，`algorithm` 为小写算法名。
版本用例也必须提供 headers，虽然 version parser 只读取 body。
checksum 的 `artifact` 是具体文件名，不是带 `{version}` 的模板。

### Validate 的阶段边界

`validate.manifest.reference_fixture` 的 `input` 内嵌完整的
[`resources.json`](../crates/manifest-metadata/tests/fixtures/resources.json) 快照副本，
仅将副本的 `manifest_version` 从当前整数 `2` 改成设计要求的字符串 `"2.0"`。
其他字段和值均保持 JSON 结构相等；原始 fixture 不变。
运行器直接消费本文件的 `input`，无需对版本做隐式迁移。

B1 尚未定义每条 invariant 的完整 `validate.*` 错误码，因此此文件暂时只有这一条正向占位用例。
B1 必须补齐重复 variant、伪造 `tag_key`、悬空 default、registry / mirror 引用、
static history、parser 可编译性和模板变量依赖等适用不变量的正反用例，使用届时确定的精确码；
本卡不预造错误码。

### Compat 的结果投影

所有兼容用例只包含一个 resource `sample` 和一个 variant `stable`。
成功解码后，运行器先验证 manifest，再为该 variant 中每个保留的已知平台产物预检 parser 支持性：
编译 remote version parser，并检查 remote checksum parser 是否受支持。
本阶段不发 HTTP 请求、不要求远端版本值，也不执行下载时的 kind 接受策略。
支持的产物记作 `{ "ok": true }`；未知 version 或 checksum parser 仅在该产物记作
`{ "error": { "code": "resolve.unsupported_parser" } }`，其他产物继续检查。
未知 parser 不得使 manifest 的解码或语义校验整体失败。

`expect.ok` 的完整投影形状为：

```json
{
  "newer_minor": true,
  "default_variants": {
    "windows-x86_64": "stable",
    "linux-amd64": "stable"
  },
  "artifacts": {
    "windows-x86_64": {
      "error": { "code": "resolve.unsupported_parser" }
    },
    "linux-amd64": { "ok": true }
  }
}
```

`default_variants` 是解码后 `resources.sample.default_variants` 的完整映射；
`artifacts` 必须恰好列出解码后 `resources.sample.variants[0].artifacts` 的全部键。
未知平台 `future-os-cpu` 必须从这两个映射中跳过。
运行器还应确认已知 resource / variant 的标签、source 和受支持产物的 wire 数据保留完整。
`newer_minor` 相对于当前已知 minor `0` 计算，`"2.7"` 为 `true`，`"2.0"` 为 `false`。
版本解码失败使用顶层 `expect.error`，不执行逐产物检查。
`compat.newer_minor.unknown_platform_and_parser` 将新 minor、未知平台和未知 parser
放在同一个 manifest 中，直接覆盖设计 G8。

### Kind 的名称与静态推导

本测试集用 `authoritative`、`parsed`、`snapshot`、`none` 表示四个 kind。
这是小写 snake_case 约定，与 metadata 中 `ResourceArtifactVersionTemplate`
的 serde `rename_all = "snake_case"` 风格一致；当前 Rust 尚未定义 `ChecksumKind`。
现有 checksum parser 的 `github-release-assets` 仍按其 kebab-case wire 名称书写。

四条用例分别断言：static release 内嵌 checksum → `authoritative`；static 无 checksum →
`none`；remote 声明 checksum 规则 → `parsed`；remote 无规则 → `none`。
static 用例只有一个 release，且等于 `latest`，不引入历史版本选择歧义。
两个 remote 用例都带有含 checksum 的 `fallback`，仍按规则是否存在分别得出 `parsed` / `none`。
`snapshot` 是显式离线 fallback 且解析版本等于 `fallback.version` 时的执行结果，
不是 §5.1 规则 3 的静态推导输出，因此这里没有期望为 `snapshot` 的用例。

## 来源与参考校验

- 规范：[design.md](../docs/spec/2026-11-06-manifest-core-parsers/design.md)
  §1.2 / §1.3、§5.1–§5.4、§5.6，以及 [S0b 卡片](../docs/spec/2026-11-06-manifest-core-parsers/tasks.md)。
- wire 字段、tag 和命名：[`crates/manifest-metadata/src/lib.rs`](../crates/manifest-metadata/src/lib.rs)。
- 正向校验快照：[`resources.json`](../crates/manifest-metadata/tests/fixtures/resources.json)。
  compat manifest 按它的 registry、resource、展开 tag、variant 和 artifact 布局手写缩小。
- 上游只读参考：`files/scripts/resource-template.ts` 的 `parseRemoteVersion`
  和 `parseRemoteChecksum`（约 1211–1280 行）。`files/` 不入库，运行器不依赖它。
- parser body、digest 和 GitHub HTML 均为确定性的合成输入；HTML 使用独立 `<li>` 资产条目和
  download 链接，不声称为抓取的真实页面。A4 再收集真实 expanded-assets fixtures 并记录来源和时间。

创建时通过 Node 原生 TS stripping 直接 import 只读参考模块，逐项比较了以下正常路径：

- `version.text.trim_default`、`version.text.trim_false_clean`
- `version.regex.capture`、`version.regex.group_zero`
- `checksum.checksum.normalized`、`checksum.gnu.exact_name`
- `checksum.bsd.matching_label`、`checksum.github_release_assets.own_digest`

一次性 Node 脚本的比较操作为
`parseRemoteVersion(input.response.body, input.parser)` 或
`parseRemoteChecksum(input.response.body, input.parser, input.artifact)`，
通过 `node:assert/strict` 与 `expect.ok` 比较，未修改上游文件。
新增 `header` parser 没有上游实现，其结果按规范和 base64 字节解码确定。
缺陷用例使用修正结果；额外执行参考函数确认两个 GitHub 陷阱会返回相邻 / 前缀资产的 digest，
BSD 错标签陷阱会被误当成配置的 SHA-256。

## 缺陷 ↔ 用例

下表覆盖 design §1.2 / §1.3 中全部带「codex 复现」标注的行，
并列出 S0b 要求的其他语义修正。GitHub 一行中的两种复现分别列出。

| 来源 / 缺陷                                                       | 用例                                            | 修正后的期望                                          |
| ----------------------------------------------------------------- | ----------------------------------------------- | ----------------------------------------------------- |
| §1.2：`alpha?x=1#f` 改写 URL query / fragment（codex 复现）       | `version.text.query_fragment`                   | `parser.invalid_version`，在生成 URL 前拒绝该 version |
| §1.3：当前资产缺 digest，却取到相邻资产的值（codex 复现之一）     | `checksum.github_release_assets.digest_missing` | `parser.digest_missing`                               |
| §1.3：`selected.zip` 命中 `prefix-selected.zip`（codex 复现之二） | `checksum.github_release_assets.exact_name`     | 只返回 `selected.zip` 自己的 digest                   |
| §1.3：`text` 的 `trim: false` 仍被无条件 trim                     | `version.text.trim_false_newline`               | `parser.invalid_version`                              |
| §1.3：BSD 不核对算法标签                                          | `checksum.bsd.mismatched_label`                 | 跳过 `SHA512` 行，`parser.not_found`                  |
| §5.3 / S0b：GNU 同名行 digest 冲突                                | `checksum.gnu.conflicting_digests`              | `parser.ambiguous`                                    |
| §5.3 / S0b：regex 的 group 超过捕获组数量                         | `version.regex.group_out_of_range`              | `parser.group_out_of_range`，编译阶段拒绝             |
| §5.1 / S0b：未声明 header 规则时隐式读取 `Digest`                 | `checksum.checksum.ignores_digest_header`       | 返回 body 的 checksum，忽略不同的 header digest       |

GitHub 精确名称陷阱将带不同 digest 的 `prefix-selected.zip` 放在 `selected.zip` 前，
两者距离小于旧实现的 2048 字符窗口；旧邻近正则会错误返回前者。
缺 digest 陷阱中，`selected.zip` 之后紧邻一个带 digest 的其他资产。
BSD 陷阱故意将 64 位、符合配置 SHA-256 长度的 digest 标成 `SHA512`；
这是针对旧实现不核对标签的畸形输入，不是合法的 SHA-512 checksum 示例。

## 数据验证

S0b 的验证是在仓库根目录用一次性 Node 脚本遍历 `conformance/**/*.json`，
逐文件 `JSON.parse`，确认顶层为数组并检查所有 `name` 全局唯一，同时打印每文件用例数。
脚本不入库，执行报告中保留完整文本。然后执行：

```sh
pnpm exec prettier --write conformance
pnpm fmt:check
```

A2 / A3 建立 Rust parser 运行器，B1 / B3 补充 validate / kind 运行器，
B4 填充 `resolve/`，C3 再让 Node 读取同一份数据并核对用例名清单。
