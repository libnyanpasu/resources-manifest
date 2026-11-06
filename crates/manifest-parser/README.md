# manifest-parser

无 IO 的资源 parser 定义与执行。版本规则先用 `RemoteVersionParser::compile`
编译，再用 `CompiledVersionParser::parse` 处理借用的 `RemoteResponse`。
`VersionLabel` 只接受 `^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$`，转换时不 trim。

`Limits` 的字段私有，默认采用设计 §5.5 的全部六项上限。`with_*` 链式方法
将请求值夹到当前值以下：超出默认值或尝试重新提高已降低的值时均保留当前值；
零也可以作为上限。模板深度和渲染输出上限放在这里，供 B2 使用。
body、header 值与 regex pattern 长度均按 UTF-8 字节数计量。
编译后的版本 parser 保存生效的 body 上限，每次 parse 在 trim 或 regex 匹配前检查。

`RemoteResponse::header_values` 按 ASCII 大小写不敏感的名称返回原始值；
`single_header` 对完全相同的重复值去重，不修改值本身。缺失报 `parser.not_found`，
不同值报 `parser.ambiguous`。版本 parser 只读取 body；header 值的大小与编码检查
由 checksum 的 `header` parser 执行。

`ParseError` 的枚举字段可直接匹配，`fields()` 提供稳定的 JSON 字段对象。
`body_too_large` 带 `{ limit, actual }`，`group_out_of_range` 带 `{ group, groups }`，
其中 `groups` 包含 group 0；`unsupported_parser` 带 `{ parser_type }`。
pattern 字节数超限也用 `parser.invalid_pattern`，带 `{ limit, actual }`；
语法或编译大小错误的同一码字段为 `{}`，保留 regex 错误作为 source。
版本的 `not_found`、`invalid_version` 字段为 `{}`；header helper 的
`not_found`、`ambiguous` 带 `{ name }`，保留调用者声明的名称。
未参与首个匹配的可选捕获组按 `parser.not_found` 处理，不尝试后续匹配。
`path()` 对 body 响应错误返回 `response.body`，对 header 错误返回
`response.headers.<ASCII 小写名称>`。规则错误使用 `parser.pattern`
（pattern 超限或编译失败）、`parser.group`（捕获组越界）和
`parser.type`（未知 parser 类型），与 conformance 输入的 `parser` 对象一致。

`RemoteChecksumParser::parse` 实现全部五种规则：

- `checksum`：trim body 后解析 `<algorithm>:<hex>`，规范化算法名和 hex。
- `gnu`：按完整原始 artifact 名匹配 GNU 列表，分别校验分隔符和行尾空格 / tab。
- `bsd`：精确匹配文件名，并按 ASCII 大小写不敏感比较算法标签。
- `github-release-assets`：从 HTML 的 `li` 资产条目读取下载链接和 digest；
  跳过脚本、样式和注释，digest 不跨嵌套条目。href 先解码 HTML 引用，
  去掉 query / fragment，再提取最后路径段并百分号解码一次后精确匹配。
- `header`：只读取显式声明的 header，解码 hex 或标准 base64；
  检查匹配 header 的单值上限，忽略 body 和其他 header。

GNU / BSD 与 GitHub 对归一化后相同的 digest 去重，冲突报 `parser.ambiguous`；
没有匹配文件名报 `parser.not_found`，GitHub 匹配条目没有 digest 报 `parser.digest_missing`。
这些错误的字段为 `{ name }`，保留 artifact 原值。非 header 规则均先检查 body 上限。

一致性运行器复用 `run_cases` 与结果比较逻辑，分别逐条执行 version 和 checksum corpus，
汇总全部失败名称，精确比较错误码、完整 fields 对象和 path。
另一个 libtest 测试递归检查全部 corpus 文件的名称唯一性，不包含 `fixtures/` 的期望清单。
