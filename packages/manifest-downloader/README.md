# @manifest/downloader

Node.js >= 24.2 的构建期下载器；选择、模板、parser 和校验策略通过
`@manifest/runtime` 调用 wasm core。运行前执行 `pnpm build:wasm`。

```sh
pnpm --filter @manifest/downloader start --manifest resources.json --resource sample --platform linux-amd64 --out ./downloads
```

包命令入口为 `manifest-download`。`--manifest` 支持本地路径和 HTTP(S) URL。
可选参数：`--variant <id>` 或
`--tags <a,b>`（互斥）、`--mirror <id>`、`--allow-parsed`、`--allow-snapshot`、
`--max-artifact-bytes <bytes>`。

默认只接受 `authoritative`。远端产物默认在请求版本和 checksum 之前拒绝，诊断中的
版本为 `unresolved`，并提示 `--allow-parsed`。允许后会在产物下载前打印解析出的版本、
kind、策略决定及原因；接受 parsed / snapshot 时打印 `WARN`。

仅传 `--allow-snapshot` 时先打印 `version=unresolved kind=snapshot` 的策略决定，
再查询远端版本并请求 core 的 snapshot 计划；core 确认后打印已解析版本的有效策略。
仅当解析出的
版本与 manifest fallback 版本完全一致且 fallback 有 checksum 时才可下载。
同时传两个允许开关时优先使用远端 parsed checksum；不自动回退到 snapshot。

所有 HTTP 请求集中在 `http.ts`，使用 `globalThis.fetch` 和 `redirect: "manual"`。
只传入 header parser 声明的头，名称统一为小写；存在时把 `headers.get(name)` 返回的
合并值作为单项交给 core。core 按逗号拆分，裁剪元素两端的 SP / HTAB 并忽略空元素；
没有元素报 `parser.not_found`，相同元素接受，不同元素报 `parser.ambiguous`。
标准 fetch 无需暴露每次出现的原始头行；未声明的响应头不会提供 checksum。

手动跟随最多 5 次重定向，每个请求（含读取 body）使用一个 `AbortSignal.timeout`
截止时间，默认 30 秒；重定向每跳各自计时。所有请求均在发出下一跳之前拒绝
HTTPS → HTTP 降级，报 `http.insecure_redirect`；仅接受无 userinfo、无 fragment 的
HTTP(S) URL。manifest 的 HTTP body 和 parser body 按流计数，最多 2 MiB；声明的
单个收到的 header 值（包括 fetch 合并值）最多 8 KiB，在 core 拆分前检查。
未消费的响应 body 会取消；超时同时中止等待响应头与 body 读取。
runtime 目前没有导出 Limits 默认值的读取接口，因此 HTTP 上限按设计 §5.5 设置。

代理依赖 Node 的 fetch 内建环境代理支持，不自行解析代理开关或管理 Agent / socket。
[Node.js CLI 文档](https://nodejs.org/api/cli.html#node_use_env_proxy1) 中的
`NODE_USE_ENV_PROXY=1` 从 Node.js 24.0.0（以及 22.21.0）起适用于 fetch，因此本包
最低版本 24.2 已可使用；[`--use-env-proxy`](https://nodejs.org/api/cli.html#--use-env-proxy)
命令行 / `NODE_OPTIONS` 开关从 24.5.0（以及 22.21.0）起提供。
[HTTP 内建代理文档](https://nodejs.org/api/http.html#built-in-proxy-support) 的 24.5.0
门槛对应 `node:http` / `node:https` 的 Agent 支持，不是 fetch 的环境变量门槛。
在进程启动时启用后，通过 `HTTP_PROXY` / `HTTPS_PROXY` 配置代理，`NO_PROXY`
指定直连主机；未启用时保持 Node 默认行为。子进程测试按环境变量与 CLI 开关的
各自版本门槛执行，验证代理路由与 NO_PROXY 绕过。

产物以流方式写入 `--out` 内的临时文件，同时计算 digest；默认最多 1 GiB，可通过
`--max-artifact-bytes` 设置正的安全整数。校验通过后以产物路径的最后一段作为文件名，
原子 rename 到输出目录；失败会删除临时文件，并保留已有输出文件。
计划生成后、请求 checksum 或产物之前，在所有平台统一拒绝 Windows 保留设备名、
非法字符和控制字符、尾随点或空格，以及空、`.`、`..` 的文件名，报 `download.invalid_filename`。

程序入口 `run(argv, { log?, timeoutMs? })` 返回最终文件路径，失败时抛出错误。
`ManifestRuntimeError` 保留 `code`、`path`、`message` 和 `fields`；CLI 将错误打印成一行
`error: <code>: <message> [<path>]` 并以非零状态退出。
HTTP 错误的 `path` 使用稳定的 `http.manifest` / `http.version` / `http.checksum` /
`http.artifact` 定位符；诊断中的 URL 去掉 userinfo 和 fragment，查询参数替换为 `?<redacted>`。

```sh
node --test "packages/manifest-downloader/*_test.ts"
```

下载测试只访问绑定在 `127.0.0.1` 随机端口的本地服务器；临时测试目录位于本包内。
