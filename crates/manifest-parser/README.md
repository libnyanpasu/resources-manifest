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
由 A5 的 checksum header parser 执行。

`ParseError` 的枚举字段可直接匹配，`fields()` 提供稳定的 JSON 字段对象。
`body_too_large` 带 `{ limit, actual }`，`group_out_of_range` 带 `{ group, groups }`，
其中 `groups` 包含 group 0；`unsupported_parser` 带 `{ parser_type }`。
pattern 字节数超限也用 `parser.invalid_pattern`，带 `{ limit, actual }`；
语法或编译大小错误的同一码字段为 `{}`，保留 regex 错误作为 source。
版本的 `not_found`、`invalid_version` 字段为 `{}`；header helper 的
`not_found`、`ambiguous` 带 `{ name }`，保留调用者声明的名称。
未参与首个匹配的可选捕获组按 `parser.not_found` 处理，不尝试后续匹配。
`path()` 对版本及其编译错误返回 `response.body`，对 header helper 错误返回
`response.headers.<ASCII 小写名称>`。

一致性运行器复用 `run_cases` 与结果比较逻辑，A3 / A5 可追加 checksum corpus
和执行闭包。当前用单个 libtest 测试逐条执行并汇总全部失败名称，避免新增测试框架依赖。
它精确比较错误码、声明的完整 fields 对象以及声明的 path。
