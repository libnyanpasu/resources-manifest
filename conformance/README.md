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
  "expect": {
    "error": {
      "code": "parser.invalid_version",
      "path": "response.body",
      "fields": {}
    }
  }
}
```

- `name` 在所有目录、所有 JSON 文件中全局唯一，使用 dotted prefix；运行器以完整名称标识用例。
  Rust 的 `corpus_case_names_are_globally_unique` 递归检查整个 `conformance/` 的 corpus JSON，
  包括尚未接入执行的目录；`fixtures/` 中的 HTML 期望清单不是 corpus，不参与名称检查。
- `input` 总是对象，各目录的形状和操作见下表。
- `expect` 恰有一个分支：`{ "ok": <结果> }` 或 `{ "error": { "code": "...", "path": "...", "fields": {} } }`。
  所有错误期望都必须声明 `path` 和完整的 `fields`，即使字段对象为空也不省略。消费者按稳定的 `code` 与结构字段进行程序化匹配，不解析消息文本。运行器比较 `code`、声明的 `fields`（完整对象结构相等）和声明的 `path`（精确字符串相等）。
  `path` 统一为从输入文档根开始的 dotted locator：以 `.` 连接字段或对象键，数组下标写作 `[<i>]`，如 `manifest_version`、`resources.<id>.variants[<i>].tags`。宿主传入参数使用 `arg.<name>`，例如 `arg.version`，parser 响应错误使用 `response.body` 或 `response.headers.<小写名称>`，规则错误按输入的 `parser` 对象定位：`parser.invalid_pattern` → `parser.pattern`、`parser.group_out_of_range` → `parser.group`、`resolve.unsupported_parser` → `parser.type`。不使用裸参数名或其他路径语法。
- 结果按 JSON 结构相等比较，对象键顺序无关，数组顺序保留。不得先 trim、规范化输入或丢弃重复 header。
- checksum 成功结果是 `ResourceChecksum` 的序列化字符串：`<algorithm>:<lowercase-hex>`。
  SHA-256 digest 使用 64 个十六进制字符（32 字节），base64 输入表示同样的 32 字节。

| 目录 / 文件              | `input` 形状与运行器操作                                                                                         | `expect.ok`                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `parsers/version.json`   | `{ parser, response: { body, headers } }`；编译 version parser，再解析响应                                       | version 字符串                       |
| `parsers/checksum.json`  | `{ parser, response: { body, headers }, artifact }`；按 artifact 文件名解析 checksum                             | checksum 字符串                      |
| `validate/manifest.json` | 完整编译产物 manifest；先消费侧解码，再调用 `validate_manifest`，不补字段、不从外部 fixture 加载或合并           | `true`，表示校验成功                 |
| `compat/manifest.json`   | 完整编译产物 manifest；按运行器的 `CURRENT_MINOR`（当前 0）宽松解码，再按下文约定逐产物检查支持性，不补字段      | 兼容性投影对象                       |
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
`checksum.header.invalid_base64` 断言 `parser.invalid_encoding`；
`checksum.header.invalid_hex` 与 `checksum.header.wrong_length` 断言 `parser.invalid_checksum`。
非 hex 字符、奇数个 hex 字符和 digest 长度不符都属于 `parser.invalid_checksum`。
单个 header 值超过单值上限（默认 8 KiB，宿主调低后以生效值为准）使用 `parser.body_too_large`，同 body 超限一样携带
`{ limit, actual }`（UTF-8 字节数）；上限一致性用例由 A2 / A5 补充。

### Parser 上限覆盖（A2）

parser 用例可以增加可选的 `input.limits` 对象。未声明的项采用默认值；
所有值都是非负整数，名称为 `body_bytes`、`header_value_bytes`、
`regex_pattern_bytes`、`regex_size_bytes`、`template_depth`、`render_output_bytes`。
运行器通过 `Limits::with_*` 调低对应上限；请求值高于当前上限时夹到当前值，
不能提高默认值。未知字段应被拒绝。此扩展也供 A3 / A5 的 checksum 用例复用。

例如 `"limits": { "body_bytes": 3 }` 配合 `"body": " v1\n"`，
应得到 `parser.body_too_large`，完整字段为 `{ "limit": 3, "actual": 4 }`，
路径为 `response.body`。这样无需在 corpus 中写入 2 MiB 的 body。
`version.text.body_limit_utf8` 还覆盖多字节字符计量；默认 2 MiB 的边界由 Rust 回归测试覆盖。

`checksum.header.raised_limits_clamped` 将 `body_bytes` 和 `header_value_bytes`
都请求为 33,554,432，搭配 8,193 字节的字面 header 值，仍须报
`parser.body_too_large`，字段为 `{ "limit": 8192, "actual": 8193 }`。
它用较小的 header 证明调高请求被夹到默认值；body 默认 2 MiB 的不可调高语义
由 Rust 的 `limits_can_only_be_lowered` 验证，无需新增 corpus 生成语法或大 body。

`version.regex.pattern_limit` 与 `version.regex.pattern_limit_utf8` 覆盖 pattern
的 UTF-8 字节数；超限使用 `parser.invalid_pattern`，字段为 `{ limit, actual }`。
`version.regex.compiled_size_limit` 用短 pattern 触发默认 1 MiB 编译上限；
`version.regex.compiled_size_lowered` 验证宿主调低的上限也生效。
regex 语法或编译大小错误的 `fields` 为 `{}`，不比较依赖库的错误消息。
版本未找到或无效时的 `fields` 也为 `{}`。

version parser 不支持 header，不把 header helper 当作 version parser 类型。
`version.text.ignores_conflicting_headers` 确认它忽略 headers；header helper 的
大小写、相同值去重、冲突、缺失及 `response.headers.<小写名称>` 路径由 Rust
集成测试验证。A5 再在 checksum corpus 中为 header parser 声明路径。

### Checksum 的阶段边界与错误字段（A3 / A4 / A5）

Rust 运行器读取整个 `parsers/checksum.json`，执行 `checksum`、`gnu`、`bsd`、
`github-release-assets`、`header` 和未知 parser type 用例，不跳过任何用例。
查看完整名称可运行 `cargo test -p manifest-parser --test conformance -- --nocapture`。

`RemoteChecksumParser::parse` 对非 `header` 规则在分派之前检查 body 的 UTF-8 字节数，
超过上限时优先返回 `parser.body_too_large`。`checksum`、`gnu`、`bsd` 都只读 body，
忽略 headers 的值、冲突和大小，不会在 body 缺失或无效时回退到 header。

`header` 只读取规则显式声明的 header，不读取或限制 body，也忽略其他 header 的大小。
按 ASCII 大小写不敏感的名称查找，先检查每个匹配值的 UTF-8 字节数，
任一值超限优先报 `parser.body_too_large`，包括重复值有冲突或编码非法的情况。
只有原始字符串完全相同的重复值才合并；大小写不同的 hex 值也视为冲突。
base64 使用 RFC 4648 标准字母表（`+`、`/`），要求规范的 `=` padding（需要时）
和零填充位；拒绝 URL-safe 字母表、缺失 padding、首尾或内部空白。
hex 同样不 trim；HTTP 宿主应在构造 `RemoteResponse` 前去除传输层首尾空白。
`not_found` 和 `ambiguous` 的 fields 为 `{ name }`，保留规则声明的名称；
`invalid_encoding` 和 `invalid_checksum` 的 fields 为 `{}`。
所有 header 错误的 path 都是 `response.headers.<ASCII 小写名称>`。
corpus 覆盖调低的上限、UTF-8 计量、编码错误和路径；Rust 回归还覆盖默认 8 KiB
header 边界与超过默认 2 MiB 的 body 不影响合法 header。

header 名称在 metadata 的 `ResourceVariantTemplate` artifacts 解码边界校验：
非空且只能含 RFC 9110 token 字符，即 ASCII 字母、数字及 ``!#$%&'*+-.^_`|~``。
编译产物保持宽松解码，A5 不因非法名称而拒绝整个 manifest；B1 可进一步语义校验。
共用 parser 类型的 Schema 不增加名称 pattern，以保持模板专属校验边界，
生成物仅新增 header 变体和 `HeaderEncoding`。

GNU 和 BSD 按 `\n` 分行，每行只移除一个末尾 `\r`。
GNU 使用 `<hex>[ \t]+[* ]?<name>[ \t]*`，按完整 artifact 名称匹配，
保留名称内部的空格、特殊字符，以及 artifact 自身的首尾空格。
BSD 使用固定分隔符 `<LABEL> (<name>) = <hex>`，名称完全相等，
算法标签按 ASCII 大小写不敏感比较，不符合配置算法的行直接跳过。
两种列表都会跳过格式不符、非 hex 或 digest 长度不符的行，即使名称相同；
这与参考实现以定长 hex 正则筛选行的行为一致。
只有找到格式和长度都正确的候选行后才参与重复值比较；
归一化后相同的 digest 合并，不同的 digest 报 `parser.ambiguous`。
没有有效候选行报 `parser.not_found`。

列表的 `parser.not_found` 和 `parser.ambiguous` 字段都是 `{ name }`，
其中 `name` 为调用者请求的 artifact 原值；`checksum` 的 `parser.invalid_checksum`
字段为 `{}`。上述错误路径统一为 `response.body`。
GitHub parser 使用轻量 `tl` 解析 HTML 的 `li` 资产条目，
下载链接 href 先解码一次 HTML 字符引用，再去掉 query / fragment，
提取最后路径段并百分号解码一次后精确匹配；不看显示文本，字面 `+` 保持原样。
支持带分号的十进制 / 十六进制数字引用，以及常见命名引用
`amp`、`quot`、`apos`、`lt`、`gt`、`nbsp`
（也接受 `AMP`、`QUOT`、`LT`、`GT`）；未知引用保留原文，不递归解码。
从根遍历资产条目时剪除 script / style / 注释；digest 读取同一条目内的
`.Truncate-text`，文本收集同样跳过 li / script / style / 注释。
若 digest 容器只含被排除的内容，视为当前条目缺少 digest。
未找到名称报 `parser.not_found`；匹配条目没有 digest 报 `parser.digest_missing`；
同名条目（或同一条目）有不同 digest 报 `parser.ambiguous`，三者 fields 都是 `{ name }`。
相同 digest 在算法名和 hex 归一化后去重。任意匹配条目没有 digest 都拒绝，
即使另一个同名条目有 digest。不支持的算法、非法 hex、长度错误或空 digest
报 `parser.invalid_checksum`，fields 为 `{}`；不校验未匹配条目的 digest。
这些错误路径均为 `response.body`；body 上限仍在 HTML 解析前检查。
若 `tl::parse` 无法解析页面，同样映射为 `parser.invalid_checksum`，
fields 为 `{}`、path 为 `response.body`；不根据 tokenizer 的消息新增错误码。
临时 `ParseError::Unsupported` 已删除；未知 type 继续使用
`ResolveParser { parser_type }`，返回 `resolve.unsupported_parser` 和保留原始 tag 的
`{ parser_type }`，路径为 `parser.type`。

真实页面、采集 URL/UTC 时间、独立期望清单及 O-3 体积比较见
[GitHub fixture README](fixtures/github-expanded-assets/README.md)。
`github_expanded_assets_fixtures` 自动遍历每份 HTML，按旁边固定的
`*.expected.json` 检查每个有 digest 的资产，并确认旧页面的无 digest 资产报错；
覆盖 4 份 fixture、219 个资产（191 个 digest）。百分号编码 fixture 明确标为合成，
真实 HTML 保持原样。运行 `cargo test -p manifest-parser --test conformance` 即可复核。

A3 再次直接调用只读参考函数，24 个正常路径成功结果与 corpus 相同。
有意差异为 S0b 已记录的 GNU 同名冲突、BSD 错误算法标签；
新增 `checksum.bsd.skips_other_algorithm` 进一步验证跳过错误标签后返回后续正确算法的 digest，
参考实现会误取第一行。已有 18 个 checksum 用例的名称、输入和期望均保持不变。

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
static 产物，以及所有 parser 都受支持的 remote 产物，其支持性结果投影为 `{ "ok": true }`，
并在同一对象的 `kind` 字段中按设计 §5.1 规则 3 记录静态推导值。
未知 version 或 checksum parser 仅在该产物记作
`{ "error": { "code": "resolve.unsupported_parser", "fields": { "parser_type": "..." } }, "kind": "none" }`，
其他产物继续检查；`none` 不被任何校验策略接受。
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
      "error": {
        "code": "resolve.unsupported_parser",
        "fields": { "parser_type": "future-version-parser" }
      },
      "kind": "none"
    },
    "linux-amd64": { "ok": true, "kind": "authoritative" }
  }
}
```

`default_variants` 是解码后 `resources.sample.default_variants` 的完整映射；
`artifacts` 必须恰好列出解码后 `resources.sample.variants[0].artifacts` 的全部键。
未知平台 `future-os-cpu` 必须从这两个映射中跳过。
运行器还应确认已知 resource / variant 的标签、source 和受支持产物的 wire 数据保留完整。
`newer_minor` 相对于运行器的 `CURRENT_MINOR`（当前为 `0`）计算；按当前值，`"2.7"` 为 `true`，`"2.0"` 为 `false`。
minor 不允许前导零；`compat.version.leading_zero` 断言 `"2.07"` 报 `manifest.invalid_version_format`。
版本解码失败使用顶层 `expect.error`，不执行逐产物检查。
`compat.newer_minor.unknown_platform_and_parser` 将新 minor、未知平台和未知 parser
放在同一个 manifest 中，直接覆盖设计 G8。

### Kind 的名称与静态推导

本测试集用 `authoritative`、`parsed`、`snapshot`、`none` 表示四个 kind。
这是设计 §5.1 固定的小写 snake_case wire 名称，Rust `ChecksumKind` 必须使用
`#[serde(rename_all = "snake_case")]`，并非仅限本测试集的命名约定。
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
- `parsers/checksum.json` 中的内嵌 GitHub HTML 是确定性的合成输入；A4 的真实页面另存于
  `fixtures/github-expanded-assets/`，来源、独立期望值和唯一合成 fixture 的说明见其 README。

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
`checksum.github_release_assets.percent_encoded_href` 是规范用例：href 的最后路径段
`foo%2Bbar.zip` 解码为 `foo+bar.zip`，显示文本刻意不同，仍返回该条目自己的 digest；
此修正语义不要求旧参考函数通过。
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
BSD 陷阱故意将包含 64 个十六进制字符（32 字节）、符合配置 SHA-256 长度的 digest 标成 `SHA512`；
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
