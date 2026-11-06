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
- `expect` 恰有一个结果分支：`ok` 或 `error`，分别为 `{ "ok": <结果> }` 或 `{ "error": { "code": "...", "path": "...", "fields": {} } }`。
  validate 用例选择 `error` 结果分支时，可另外声明 `expect.additional_errors` 数组，列出其余所有错误。
  所有错误期望都必须声明 `path` 和完整的 `fields`，即使字段对象为空也不省略。消费者按稳定的 `code` 与结构字段进行程序化匹配，不解析消息文本。运行器比较 `code`、声明的 `fields`（完整对象结构相等）和声明的 `path`（精确字符串相等）。
  `path` 统一为从输入文档根开始的 dotted locator：以 `.` 连接字段或对象键，数组下标写作 `[<i>]`，如 `manifest_version`、`resources.<id>.variants[<i>].tags`。宿主传入参数使用 `arg.<name>`，例如 `arg.version`，parser 响应错误使用 `response.body` 或 `response.headers.<小写名称>`，规则错误按输入的 `parser` 对象定位：`parser.invalid_pattern` → `parser.pattern`、`parser.group_out_of_range` → `parser.group`、`resolve.unsupported_parser` → `parser.type`。不使用裸参数名或其他路径语法。
- 结果按 JSON 结构相等比较，对象键顺序无关，数组顺序保留。不得先 trim、规范化输入或丢弃重复 header。
  validate 不增加合成的 aggregate 错误码。运行器把首条与 additional_errors 合并为多重集合，
  精确比较全部 code、path、fields，错误排列不影响结果，但不会忽略多余、缺失或重复的错误。
- checksum 成功结果是 `ResourceChecksum` 的序列化字符串：`<algorithm>:<lowercase-hex>`。
  SHA-256 digest 使用 64 个十六进制字符（32 字节），base64 输入表示同样的 32 字节。

| 目录 / 文件              | `input` 形状与运行器操作                                                                                         | `expect.ok`                            |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `parsers/version.json`   | `{ parser, response: { body, headers } }`；编译 version parser，再解析响应                                       | version 字符串                         |
| `parsers/checksum.json`  | `{ parser, response: { body, headers }, artifact }`；按 artifact 文件名解析 checksum                             | checksum 字符串                        |
| `validate/manifest.json` | 完整编译产物 manifest；先消费侧解码，再调用 `validate_manifest`，不补字段、不从外部 fixture 加载或合并           | `true`，表示校验成功                   |
| `compat/manifest.json`   | 完整编译产物 manifest；按运行器的 `CURRENT_MINOR`（当前 0）宽松解码，再按下文约定逐产物检查支持性，不补字段      | 兼容性投影对象                         |
| `kind/artifact.json`     | artifact template wire object：`{ artifact, source?, version }`；调用 `artifact_checksum_kind`，无请求或响应输入 | kind 字符串                            |
| `resolve/templates.json` | B2：明确的产物渲染或通用模板渲染，见下文                                                                         | `{ artifact, canonical_url }` 或字符串 |

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

### Resolve 模板用例（B2）

`resolve/templates.json` 由 `crates/manifest-core/tests/resolve.rs` 执行，所有用例
继续受 parser 运行器的递归全局名称唯一性检查覆盖。B4 reference golden 见下文。

产物用例 input 为 `{ manifest, resource, variant, platform, version?, source?, limits? }`：
`manifest` 是完整编译产物，或恰为 `{ "snapshot": "resources.json" }`，后者加载
`crates/manifest-metadata/tests/fixtures/resources.json`，不接受其他路径，也不合并覆盖字段。
先消费侧解码并 `validate_manifest`，再按明确 resource / variant / platform 获取产物。
`version` 和 `source` 是宿主 Literal，version 可来自 remote parser；static 默认 latest，
remote 必须传 version；匹配 static release 或 remote fallback 时应用其 source。
`expect.ok` 恰为 `{ artifact, canonical_url }`，错误仍精确比较 code、path、完整 fields。

通用模板 input 为 `{ template, values, path, limits? }`，每个 values 值恰为
`{ "authored": "..." }` 或 `{ "literal": "..." }`；`expect.ok` 为字符串。
这些用例直接调用 `render_template`，包括已被 B1 提前阻止的未知依赖，以及
未来 mirror 使用的含 URL 标点/花括号的 Literal；通用 Literal 不实施宿主字符集校验。
传入的 path 用于模板内部错误。本文件使用 `arg.template`，不伪造 manifest locator。

两种 input 的 `limits` 只接受 `template_depth`、`render_output_bytes`，均通过
`Limits::with_*` 调低，不能调高；未声明使用默认值。深度根为 0，进入 Authored 加 1，
Literal 不加深度；输出按 UTF-8 字节，在每次追加前检查，actual 为首次超限追加的
前缀与片段总字节数。测试包含默认与调低上限的正反边界，以及调高请求被夹到默认值。

`resolve.host_version_injection` 在 `arg.version` 拒绝 `alpha?x=1#f`；
`resolve.authored_release` 验证 `release: "{version}"` 可递归展开。
其余用例覆盖每层优先级、Literal 不重新扫描、DFS、所有 template / url 错误、
URL 规范化、保留已有百分号与 `+`、允许作者 query、拒绝空 userinfo / fragment。
快照回归逐 resource × variant × platform × static history 渲染全部 48 个 URL，
确认每个 canonical URL 均不含 `?` 或 `#`；不访问网络。

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
名称起点最多比较两处：无标记时按 artifact 的前导空白长度回退，有 `*` 标记时从其后开始；
空格标记等价于分隔空白的一部分。全空白名称在分隔空白中线性定位。
每个候选必须以完整 artifact 开头，余下只能是空格或制表符；不会先 trim 名称或枚举所有出现位置，
整行解析为线性时间。
BSD 使用固定分隔符 `<LABEL> (<name>) = <hex>`，名称完全相等，
算法标签按 ASCII 大小写不敏感比较，不符合配置算法的行直接跳过。
两种列表都会跳过格式不符、非 hex 或 digest 长度不符的行，即使名称相同；
这与参考实现以定长 hex 正则筛选行的行为一致。
只有找到格式和长度都正确的候选行后才参与重复值比较；
归一化后相同的 digest 合并，不同的 digest 报 `parser.ambiguous`。
没有有效候选行报 `parser.not_found`。
两种列表均不接受空 artifact 名称：即使输入为 `<hex>  ` 或 `SHA256 () = <hex>`，
请求 `artifact: ""` 仍返回 `parser.not_found`（`fields: { name: "" }`）；
仅含空格或制表符的非空名称仍按完整名称匹配。

列表的 `parser.not_found` 和 `parser.ambiguous` 字段都是 `{ name }`，
其中 `name` 为调用者请求的 artifact 原值；`checksum` 的 `parser.invalid_checksum`
字段为 `{}`。上述错误路径统一为 `response.body`。
GitHub parser 使用轻量 `tl` 解析 HTML 的 `li` 资产条目，
下载链接 href 先解码一次 HTML 字符引用，再去掉 query / fragment，
提取最后路径段并百分号解码一次后精确匹配；不看显示文本，字面 `+` 保持原样。
字符引用采用明确的子集，以控制 wasm 体积，不引入完整 WHATWG 命名引用表。
支持的引用遵循 [WHATWG 属性值规则](https://html.spec.whatwg.org/multipage/parsing.html#named-character-reference-state)：

- 十进制 `&#N;` 与十六进制 `&#xH;` / `&#XH;` 消费连续有效数字，末尾分号可省略。
  按 [数字引用规则](https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state)
  应用 0x80–0x9F 的 C1 替换表（例如 `&#128;` → `€`，表中未列出的值保持原码点）；
  0、代理码点及大于 U+10FFFF 的值（含整数溢出）替换为 U+FFFD。
- 带分号的命名引用支持 `amp`、`lt`、`gt`、`quot`、`apos`、`nbsp`，以及 ASCII 标点引用
  `num`、`sol`、`colon`、`quest`、`equals`、`period`、`plus`、`percnt`、`excl`、`commat`、
  `lpar`、`rpar`、`lsqb`、`rsqb`、`lowbar`、`Hat`、`grave`、`verbar`、`tilde`；
  另支持 `AMP`、`LT`、`GT`、`QUOT`。
- 仅 `amp`、`lt`、`gt`、`quot` 及其上述大写形式支持省略分号，且紧随字符必须不是
  ASCII 字母、数字或 `=`（属性值结束也可）。因此 `a&amp.zip` 解码为 `a&.zip`，
  `a&ampx` 和 `a&amp=b` 保持原文。

其余引用（如 `&copy;`）保留原文；单次线性扫描，不递归解码。
解码后先从完整 href 去掉 query / fragment，再要求路径包含 `/releases/download/`；
仅在 query / fragment 中出现该片段不算下载链接，`&sol;` 则可改变实际路径边界。
从根遍历资产条目时剪除 script / style / 注释；digest 读取同一条目内的
`.Truncate-text`，文本收集同样跳过 li / script / style / 注释。
若 digest 容器只含被排除的内容，视为当前条目缺少 digest。
若 `.Truncate-text` 内有效 digest 文本与注释或 script 等被排除内容并列，
仍读取保留的文本并返回 digest。
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

正向用例只要求校验成功。Rust 运行器额外断言 manifest 往返保留（round-trip）与
`newer_minor` 标记正确；这些属于实现侧检查，不是 corpus 契约的一部分。

`validate.manifest.reference_fixture` 的 `input` 内嵌完整的
[`resources.json`](../crates/manifest-metadata/tests/fixtures/resources.json) 快照副本，
与当前快照在 JSON 结构上相等；两者均为字符串版本 `"2.0"`。
运行器直接消费本文件的 `input`，无需对版本做隐式迁移，另有直接加载快照的回归测试。

B1 运行器位于 `crates/manifest-core/tests/conformance.rs`，递归加载 validate / compat
目录下所有 JSON 文件，不跳过用例。`validate/invariants.json` 包含 61 条用例，
加上既有快照共 62 条（21 正、41 反），覆盖 design §7.1 的 10 组不变量，满足 G6。
下表名称均省略 `validate.` 前缀；对应稳定错误码与完整字段见
[design §5.6](../docs/spec/2026-11-06-manifest-core-parsers/design.md#56-错误码)。

| 不变量 / code 后缀  | 正向用例                                                                    | 反向用例示例                                                                                                                                                                                         |
| ------------------- | --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ids`               | `ids.valid`                                                                 | `ids.invalid_variant`、`ids.duplicate_catalog`、`ids.duplicate_variant_tag`、`ids.duplicate_variant`                                                                                                 |
| `tag_key`           | `tag_key.canonical`                                                         | `tag_key.forged`、`tag_key.unsorted`                                                                                                                                                                 |
| `expanded_tag`      | `expanded_tag.consistent`                                                   | `expanded_tag.unknown`、`expanded_tag.mismatched_name` / `group` / `description`                                                                                                                     |
| `tag_group`         | `tag_group.empty_and_distinct`                                              | `tag_group.conflict`                                                                                                                                                                                 |
| `tag_combination`   | `tag_combination.distinct`                                                  | `tag_combination.duplicate`                                                                                                                                                                          |
| `default_variant`   | `default_variant.multiple_configured`、`default_variant.single_inferred`    | `default_variant.missing` / `unknown` / `unsupported_platform`                                                                                                                                       |
| `reference`         | `reference.existing`                                                        | `reference.resource_registry_missing`、`reference.remote_registries_missing`、`reference.mirror_registry_missing`                                                                                    |
| `static_history`    | `static_history.latest_not_first`                                           | `static_history.latest_missing`、`static_history.duplicate`                                                                                                                                          |
| `parser_compile`    | `parser_compile.known`、`parser_compile.unsupported_accepted`               | `parser_compile.pattern` / `group` / `pattern_limit` / `header_name`                                                                                                                                 |
| `template_variable` | `template_variable.complete`、`template_variable.release_and_remote_scopes` | `template_variable.artifact_missing` / `registry_missing` / `nested_missing` / `release_missing` / `remote_source_missing` / `remote_artifact_missing` / `remote_artifact_absent` / `mirror_missing` |

`all_violations.collected` 同时触发七条跨不变量违规，确认没有提前返回；
重复 tag 的用例也同时断言重复 ID 与非规范 key。
`template_variable.cycle_deferred` 确认 B1 只检查来源完整，环由 B2 处理。
`artifact_alias_complete` / `remote_artifact_alias_complete` 确认 registry 变量可引用
生成后的 artifact；`artifact_source_dependency_missing` 确认 artifact 模板实际使用的
source 依赖仍逐项检查，错误定位到包含缺失引用的 source 字段。
未实例化的 registry 可保留上下文变量。B1 不实际渲染、不验证 URL 或值可得的阶段；
内置 `version` 计作运行时来源，因此已有 compat 用例保持原样。
checksum 没有 compile API，B1 只预检 header 名称，其余配置约束已在类型内；
version 的 compile 使用默认 Limits，错误包为 `validate.parser_compile` 并保留原码和字段。
manifest wire 不含具名 mirror 引用，mirror 的 registry 允许列表必须引用已知 registry。

运行 `cargo test -p manifest-core --test conformance -- --nocapture` 可打印完整用例名和数量。
原 `validate/manifest.json` 与 `compat/manifest.json` 的名称、输入和期望均未改动；
parser 运行器的全局名称检查递归覆盖新增文件。

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

`kind/artifact.json` 的八条用例断言：static release 内嵌 checksum → `authoritative`；static 无 checksum →
`none`；remote 声明 checksum 规则 → `parsed`；remote 无规则 → `none`。
static 还覆盖 latest 不在首位、历史 checksum 与 latest 不同的情形，静态展示始终取 latest。
两个 remote 用例都带有含 checksum 的 `fallback`，仍按规则是否存在分别得出 `parsed` / `none`。
`snapshot` 是显式离线 fallback 且解析版本等于 `fallback.version` 时的执行结果，
不是 §5.1 规则 3 的静态推导输出，因此 artifact 静态用例没有期望为 `snapshot` 的条目。
未知 version 或 checksum parser 均为 `none`。

### Kind、选择与计划用例（B3）

`crates/manifest-core/tests/conformance.rs` 递归执行 `kind/` 的全部 JSON，当前共 86 条，
其中 B3 新增 81 条。compat 投影也改为直接调用 core 的静态 kind 与 version 计划，
同时断言逐产物支持性；未知 parser 产物还检查全部策略组合都拒绝 none。

| 文件                  | input / 操作                                                                                                | expect.ok                              |
| --------------------- | ----------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `kind/artifact.json`  | artifact wire object，无 operation；静态推导                                                                | kind 字符串                            |
| `kind/policy.json`    | `{ operation: "policy", kind, policy }`，或 `digest_mismatch` 加 algorithm/expected/actual                  | true，或标准错误                       |
| `kind/selection.json` | `{ operation: "select" 或 "list", manifest, resource, platform, variant_id?, tags? }`                       | `{ variant_id, kind }` 或其数组        |
| `kind/plans.json`     | 同上公共字段，operation 为 `version` / `download`，可加 version/source/mirror/checksum_mode/limits/response | 完整序列化计划，或 `{ plan, version }` |
| `kind/finish.json`    | operation 为 `finish_checksum`，先计划下载，再用 response 完成 checksum                                     | `{ checksum, kind: "parsed" }`         |

manifest 必须完整内嵌，不合并字段；先解码和 validate，再选择。所有名称继续由 parser
运行器检查全局唯一。`tags` 为精确组合，顺序无关，空数组和重复 tag 拒绝；多条件选择必须一致。
列表保持 variant 原顺序，只包含目标平台可用的产物；没有候选时列表为空，选择则报错。

计划字段结构与 [design §7.3](../docs/spec/2026-11-06-manifest-core-parsers/design.md#73-b3-选择计划与校验策略)
一致，optional 请求 / checksum 序列化为 null，native compiled parser 和 Limits 不导出。
`version` 操作给出 response 时实际运行计划中的 compiled parser，并比较 `{ plan, version }`。
`finish_checksum` 保持 parser 规则的显式来源，不从其他 header 推断；列表 parser 匹配真实下载
产物名，计划中的虚拟 checksums.txt 不作为匹配名。response.headers 保持二元组数组格式。
limits 可调低 body_bytes/header_value_bytes/render_output_bytes/regex_pattern_bytes，缺省为默认。

`checksum_mode` 缺省或 `remote` 为在线策略；`snapshot` 是显式离线请求，版本必须匹配
fallback 且存在内嵌 checksum。fallback checksum 不影响静态 kind；在线计划也不会自动使用它。
static 下载按实际选择的 release 取 kind。未知 parser 在 snapshot 模式同样拒绝。
mirror 用例覆盖缺省、空、匹配、不匹配的 registries，以及 version / checksum 使用独立 registry
时的限制，确认 B1 只检查 mirror 允许的 registry 作用域。组件替换、url_encoded、URL 安全校验
与内置变量优先级也精确比较。策略用例穷举四个 kind × 两个 bool 开关的全部 16 种组合。

### Resolve 参考差分（B4）

`resolve/reference-golden.json` 保持上述命名用例格式：input 为
`{ manifest: { snapshot: "resources.json" }, resource, platform, variant, version }`，
expect.ok 为 `{ artifact, canonical_url, download_url }`。对象键递归按码点排序，
数组按 resource、platform 排序，使用 LF 和 Prettier JSON 格式。

参考来源为上游 [libnyanpasu/clash-nyanpasu](https://github.com/libnyanpasu/clash-nyanpasu)
的 `scripts/resource-template.ts`，本次使用只读 vendored 副本
`files/scripts/resource-template.ts`。`files/` 未附带可确定上游 commit 的 Git 元数据；
副本的 SHA-256 为
`0a02398a48d35465e4cf23e54abf8777d2d0f4b8a5543f033d716c8974069213`。
参考模块没有运行时 import；使用 Deno `--no-config --no-lock`，避免其
`files/scripts/deno.jsonc` 中的自动 node_modules 配置产生写入。

参考 resolver 仍要求整数 manifest_version 2；脚本仅在内存副本中适配该字段，
对应 design V-9，不改快照的 `"2.0"`。每个资源的平台集合取所有 variant 的 artifact
平台并集，通过参考实现的默认选择解析。static 不传 version，使用 latest；remote
显式传 fallback.version，缺少默认产物或 fallback 立即失败。未选择 mirror，两个 URL
必须相等。当前快照覆盖 3 个资源 × 6 个平台，共 18 个默认组合（18 static、0 remote），
54 项产物名 / URL 字节比较全部一致，无新增偏差。

`crates/manifest-core/tests/reference_golden.rs` 对每项通过公开 API
`select_variant`（默认选择）和 `plan_download`（无 mirror）比较结果，并检查覆盖集合
与快照全部 resource/platform 完全相同、无重复，以及 variant 和版本选择一致。
`resolve.rs` 的 B2 模板运行器明确排除此文件，交给上述独立差分测试执行；
parser 的全局名称检查仍覆盖它，不跳过任何 golden 条目。

在仓库根目录使用 Git Bash 执行以下完整命令重建。一次性脚本写入临时目录，
不入库；只读参考无需联网，测试本身也不依赖 Deno 或 `files/`。

```bash
printf '%s\n' '0a02398a48d35465e4cf23e54abf8777d2d0f4b8a5543f033d716c8974069213  files/scripts/resource-template.ts' | sha256sum --check
b4_scratch="$(mktemp -d)"
cat > "$b4_scratch/generate.ts" <<'TS'
const root = new URL(`file:///${Deno.cwd().replaceAll("\\", "/")}/`);
const { RESOURCE_MANIFEST_VERSION, resolveResourceDownload } = await import(
  new URL("files/scripts/resource-template.ts", root).href
);
const snapshot = JSON.parse(
  await Deno.readTextFile(
    "crates/manifest-metadata/tests/fixtures/resources.json",
  ),
);
if (snapshot.manifest_version !== "2.0") {
  throw new Error("expected snapshot manifest_version 2.0");
}
const manifest = { ...snapshot, manifest_version: RESOURCE_MANIFEST_VERSION };
const cases = [];
let remote = 0;
for (const resource of Object.keys(snapshot.resources).sort()) {
  const definition = snapshot.resources[resource];
  const platforms = [...new Set<string>(
    definition.variants.flatMap((variant: { artifacts: object }) =>
      Object.keys(variant.artifacts)
    ),
  )].sort();
  for (const platform of platforms) {
    const variant = definition.default_variants[platform];
    const selected = definition.variants.find(
      (entry: { id: string }) => entry.id === variant,
    );
    const artifact = selected?.artifacts[platform];
    if (!artifact) throw new Error(`${resource}/${platform}: missing default`);
    const isRemote = artifact.version.type === "remote";
    const version = isRemote
      ? artifact.version.fallback?.version
      : artifact.version.latest;
    if (!version) throw new Error(`${resource}/${platform}: missing version`);
    const result = resolveResourceDownload(manifest, resource, {
      platform,
      ...(isRemote ? { version } : {}),
    });
    if (result.variant.id !== variant || result.version !== version) {
      throw new Error(`${resource}/${platform}: unexpected selection`);
    }
    if (result.canonical_url !== result.download_url) {
      throw new Error(`${resource}/${platform}: unexpected mirror`);
    }
    cases.push({
      name: `resolve.reference.${resource}.${platform}`,
      input: {
        manifest: { snapshot: "resources.json" },
        resource,
        platform,
        variant,
        version,
      },
      expect: {
        ok: {
          artifact: result.artifact,
          canonical_url: result.canonical_url,
          download_url: result.download_url,
        },
      },
    });
    if (isRemote) remote += 1;
  }
}
function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
        .map(([key, entry]) => [key, sorted(entry)]),
    );
  }
  return value;
}
await Deno.writeTextFile(
  "conformance/resolve/reference-golden.json",
  `${JSON.stringify(sorted(cases), null, 2)}\n`,
);
console.log(
  `${Object.keys(snapshot.resources).length} resources, ${cases.length} entries (${remote} remote, ${cases.length - remote} static)`,
);
TS
deno run --no-config --no-lock \
  --allow-read=files/scripts/resource-template.ts,crates/manifest-metadata/tests/fixtures/resources.json \
  --allow-write=conformance/resolve "$b4_scratch/generate.ts"
pnpm exec prettier --write conformance/resolve/reference-golden.json
```

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
B2 填充 `resolve/templates.json`、B4 增加参考差分，C3 再让 Node 读取同一份数据并核对用例名清单。
