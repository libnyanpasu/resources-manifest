# GitHub expanded-assets fixtures（A4）

真实 HTML 通过 `curl.exe -fLsS --max-time 40 <URL> -o <file>` 保存响应 body，
未经格式化或裁剪；`.prettierignore` 排除本目录的 HTML。时间为下载完成时的 UTC。

| 文件                     | 来源 URL                                                                      | 采集时间（UTC）                              | 覆盖                                                                                                |
| ------------------------ | ----------------------------------------------------------------------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `mihomo-alpha.html`      | https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha | 2026-11-06T18:08:11Z                         | 128 个下载资产，全部有 sha256 digest；真实滚动预发布页面                                            |
| `ripgrep-old.html`       | https://github.com/BurntSushi/ripgrep/releases/expanded_assets/14.1.1         | 2026-11-06T18:08:13Z                         | 28 个下载资产，全部没有 HTML digest；`.tar.gz` 与 `.tar.gz.sha256` 名称有前缀关系                   |
| `llvm.html`              | https://github.com/llvm/llvm-project/releases/expanded_assets/llvmorg-21.1.0  | 2026-11-06T18:08:56Z                         | 61 个下载资产，全部有 sha256 digest；`.tar.xz` 与 `.tar.xz.sig` 前缀关系；`clang+llvm-…` 的字面 `+` |
| `synthetic-encoded.html` | **合成，无真实来源 URL**；结构取自上面的 LLVM 页面                            | 2026-11-06T18:09:36Z（创建时间，非采集时间） | `%2B`、`%20` 解码；显示文本不同于文件名                                                             |

未找到 href 最后路径段包含 `%2B` 或 `%20` 的真实页面。还检查了
https://github.com/astral-sh/python-build-standalone/releases/expanded_assets/20250818
（2026-11-06T18:08:18Z，647 个下载资产）及旧版本 `20210228`
（2026-11-06T18:08:29Z）；较新 Python 页面和 LLVM 页面都把文件名里的 `+`
直接写进 href，未百分号编码。因此没有把这些页面改写成“真实”夹具，而是单独提供最小合成夹具：
第一个条目保留 LLVM 文件名与真实 digest，将 href 的 `+` 改为 `%2B`，显示文本换为无关标签；
第二个条目的名称、`%20` href 和 digest 均为手写测试数据。
Python 页面只用于搜索，没有收入夹具；较新页面的 2,701,833 字节还超过默认 2 MiB body 上限。

每个 HTML 旁的 `*.expected.json` 是固定资产清单，包含 `{ name, checksum }`，
无 digest 时 `checksum` 为 `null`。仅包括 `/releases/download/` 链接，不包括 GitHub 源码归档。
清单由一次性 Python 标准库 `html.parser.HTMLParser` 独立提取：按 `li` 读取链接，
用 `urllib.parse.urlsplit` 和 `unquote` 解码名称，读取 `span.Truncate-text` 的文本，
并核对同一条目的 `clipboard-copy[value]`（存在时）。不调用 Rust 实现或原型。
清单入库后，测试直接使用这些固定值，禁止运行时从被测解析器生成期望。
共 219 个下载资产，其中 191 个有 digest、28 个无 digest。

## O-3 原型测量

原型位于仓库外 `%TEMP%/manifest-a4-proto`，没有加入 workspace。
两种原型均遍历 `li`，从条目内的下载链接 href 解码文件名，从
`.Truncate-text` 读取 digest，返回 `Vec<(String, Option<String>)>`。
native 运行逐条比较四份固定清单，包括无 digest 的条目；不使用网络作为测试输入。

| 候选                                                                           | 正确性                        | release `.wasm` 字节数 | 相对基线增量（字节） |
| ------------------------------------------------------------------------------ | ----------------------------- | ---------------------: | -------------------: |
| 相同 cdylib，不启用解析器（返回空 Vec）                                        | 不适用                        |                     80 |                    0 |
| 轻量 `tl 0.7.8` + `percent-encoding 2.3.2`                                     | 4/4 fixture、219/219 条目正确 |                 66,004 |               65,924 |
| 完整 DOM `scraper 0.29.0`（关闭默认 features，`html5ever 0.40.1`）+ 同一解码器 | 4/4 fixture、219/219 条目正确 |                619,134 |              619,054 |

工具链：`rustc 1.101.0-nightly (249c1e4e3 2026-11-05)`，host
`x86_64-pc-windows-msvc`，target `wasm32-unknown-unknown`。
同一 crate 的 `[lib] crate-type = ["cdylib", "rlib"]`，保留一个
`#[unsafe(no_mangle)] pub fn extract(body: &str) -> Vec<(String, Option<String>)>`
导出以防解析代码被删除。三种构建只切换解析器 feature；
release profile 均为 `opt-level = "z"`、`lto = true`、`codegen-units = 1`、
`panic = "abort"`、`strip = true`。大小是原始 wasm 文件长度，未运行 wasm-opt 或 gzip。
这是最小提取原型的增量，不是整个 manifest wasm 的体积预算。

顺序执行（从不并行构建，均 `-j 2`）：

```powershell
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release
cargo run -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --features light
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release --features light
cargo run -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --features dom
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release --features dom
```

选择 `tl`：在这些真实页面及编码合成输入上结果相同，增量比完整 DOM 少 553,130 字节。
它不提供完整 HTML5 错误恢复；当前契约针对 GitHub 的 `li` 资产条目，不要求浏览器级页面修复。
实际实现还负责算法/长度校验、重复条目冲突及稳定错误，原型只测最小提取。
