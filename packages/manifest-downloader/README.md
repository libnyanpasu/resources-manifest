# @manifest/downloader

A build-time downloader for Node.js >= 24.2. Selection, templates, parsers, and
verification policy call the wasm core through `@manifest/runtime`. Run
`pnpm run build:wasm` first.

```sh
pnpm --filter @manifest/downloader run start --manifest resources.json --resource sample --platform linux-amd64 --out ./downloads
```

The package command is `manifest-download`. `--manifest` accepts a local path or
HTTP(S) URL. Optional arguments are `--variant <id>` or `--tags <a,b>` (mutually
exclusive), `--mirror <id>`, `--allow-parsed`, `--allow-snapshot`, and
`--max-artifact-bytes <bytes>`.

## Verification policy

The default accepts only `authoritative`. Remote artifacts are rejected before
version or checksum requests, with `version=unresolved` and guidance to use
`--allow-parsed`. When allowed, the downloader prints the resolved version, kind,
effective policy decision, and reason before downloading the artifact. Accepting
parsed or snapshot checksums prints `WARN`.

With only `--allow-snapshot`, it first prints the anticipated policy decision with
`version=unresolved kind=snapshot`, then fetches the remote version and requests
core's snapshot plan. After core confirms it, the resolved version and effective
policy are printed. Download is allowed only when the parsed version exactly
matches the fallback version and the fallback contains a checksum. With both
allow flags, remote parsed checksums take precedence; there is no automatic
snapshot fallback.

## HTTP behavior

All HTTP requests live in `http.ts`, using `globalThis.fetch` with
`redirect: "manual"`. Only headers declared by a header parser are passed to core,
with lowercase names. If present, the combined `headers.get(name)` value is passed
as one entry. Core splits comma lists, trims SP/HTAB around elements, and ignores
empty elements: no elements gives `parser.not_found`, identical elements are
accepted, and different elements give `parser.ambiguous`. Fetch need not expose
every original header line; undeclared headers cannot supply checksums.

Requests follow at most five redirects manually. Each request, including body
consumption, has one `AbortSignal.timeout` deadline, defaulting to 30 seconds;
each redirect hop has its own deadline. Every request rejects HTTPS-to-HTTP
downgrades before sending the next hop with `http.insecure_redirect`. Only HTTP(S)
URLs without userinfo or fragments are accepted. Manifest and parser bodies are
stream-counted with a 2 MiB limit. Each declared received header value, including
a fetch-combined value, is limited to 8 KiB before core splits it. Unconsumed bodies
are canceled; timeout aborts both header waits and body reads. Runtime does not yet
export limit-default accessors, so host HTTP limits follow design §5.5.

### Environment proxies

Proxy routing uses Node fetch's built-in environment-proxy support; the downloader
does not interpret proxy switches or manage Agents/sockets. As recorded in the
[Node CLI documentation](https://nodejs.org/api/cli.html#node_use_env_proxy1),
`NODE_USE_ENV_PROXY=1` applies to fetch from Node 24.0.0 (also 22.21.0), covering
this package's 24.2 minimum. The
[`--use-env-proxy`](https://nodejs.org/api/cli.html#--use-env-proxy) CLI/`NODE_OPTIONS`
switch is available from 24.5.0 (also 22.21.0). The 24.5.0 threshold in the
[HTTP proxy documentation](https://nodejs.org/api/http.html#built-in-proxy-support)
is for `node:http`/`node:https` Agent support, not fetch's environment-variable
threshold.

Enable support at process startup, then configure `HTTP_PROXY`/`HTTPS_PROXY` and
direct hosts through `NO_PROXY`. Without opt-in, Node's default behavior remains.
Subprocess tests respect the separate environment-variable/CLI version thresholds
and verify proxy routing and NO_PROXY bypass.

## Files and diagnostics

Artifacts stream into temporary files inside `--out` while calculating the digest.
The default cap is 1 GiB; `--max-artifact-bytes` accepts a positive safe integer.
After verification, the last segment of the artifact path becomes the output
filename and is atomically renamed into place. Failures delete temporary files
and preserve existing output.

After planning, before checksum or artifact requests, every platform rejects
Windows reserved device names, illegal/control characters, trailing dots/spaces,
and empty, `.`, or `..` filenames with `download.invalid_filename`.

`run(argv, { log?, timeoutMs? })` returns the final file path or throws.
`ManifestRuntimeError` preserves `code`, `path`, `message`, and `fields`. The CLI
prints one line, `error: <code>: <message> [<path>]`, and exits nonzero.
HTTP paths are stable `http.manifest`, `http.version`, `http.checksum`, or
`http.artifact` locators. Diagnostic URLs remove userinfo and fragments and replace
queries with `?<redacted>`.

## Tests

```sh
node --test "packages/manifest-downloader/*_test.ts"
```

Download tests use only local servers bound to `127.0.0.1` on ephemeral ports.
Temporary test directories are inside this package.
