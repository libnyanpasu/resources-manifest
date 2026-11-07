# GitHub expanded-assets fixtures (A4)

Real HTML responses were saved with
`curl.exe -fLsS --max-time 40 <URL> -o <file>`, without formatting or trimming.
`.prettierignore` excludes these HTML files. Capture times are UTC download
completion times.

| File                     | Source URL                                                                    | Captured at (UTC)                            | Coverage                                                                                                         |
| ------------------------ | ----------------------------------------------------------------------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `mihomo-alpha.html`      | https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha | 2026-11-06T18:08:11Z                         | 128 download assets, all with sha256 digests; a real rolling prerelease page.                                    |
| `ripgrep-old.html`       | https://github.com/BurntSushi/ripgrep/releases/expanded_assets/14.1.1         | 2026-11-06T18:08:13Z                         | 28 download assets, all without HTML digests; `.tar.gz` names prefix `.tar.gz.sha256` names.                     |
| `llvm.html`              | https://github.com/llvm/llvm-project/releases/expanded_assets/llvmorg-21.1.0  | 2026-11-06T18:08:56Z                         | 61 download assets, all with sha256 digests; `.tar.xz`/`.tar.xz.sig` prefixes and literal `+` in `clang+llvm-…`. |
| `synthetic-encoded.html` | **Synthetic; no real source URL.** Structure taken from the LLVM page above.  | 2026-11-06T18:09:36Z (creation, not capture) | `%2B`/`%20` decoding and display text differing from the filename.                                               |

No real page with `%2B` or `%20` in the final href path segment was found.
The search also checked
https://github.com/astral-sh/python-build-standalone/releases/expanded_assets/20250818
(2026-11-06T18:08:18Z, 647 download assets) and its old `20210228` release
(2026-11-06T18:08:29Z). The newer Python and LLVM pages use literal `+` in hrefs.
Rather than rewriting them as supposedly real fixtures, a separate minimal
synthetic fixture was created. Its first entry retains LLVM's filename and real
digest, replaces href `+` with `%2B`, and uses unrelated display text; the second
entry's name, `%20` href, and digest are handwritten. The Python pages were search
inputs only and are not fixtures; the newer page's 2,701,833 bytes also exceed the
default 2 MiB body limit.

Each adjacent `*.expected.json` is a fixed asset list of `{ name, checksum }`, with
null checksums when no digest exists. Lists include only `/releases/download/`
links, excluding GitHub source archives. They were independently extracted with
a one-off Python standard-library `html.parser.HTMLParser`: links by `li`, names
decoded with `urllib.parse.urlsplit`/`unquote`, text from `span.Truncate-text`, and
cross-checks against the same entry's `clipboard-copy[value]` when present. Neither
the Rust implementation nor the prototype produced these expectations. Tests read
the checked-in values; never generate expectations at runtime with the parser
under test. There are 219 download assets: 191 with digests and 28 without.

## O-3 prototype measurements

The prototype lived outside the repository at `%TEMP%/manifest-a4-proto` and was
not added to the workspace. Both candidates traversed `li` entries, decoded
filenames from download hrefs, read `.Truncate-text` digests, and returned
`Vec<(String, Option<String>)>`. Native runs compared all four fixed lists,
including digest-less entries; network responses were not test inputs.

| Candidate                                                                                | Correctness                   | Release `.wasm` bytes | Increase over baseline (bytes) |
| ---------------------------------------------------------------------------------------- | ----------------------------- | --------------------: | -----------------------------: |
| Same cdylib, parser disabled (empty Vec)                                                 | Not applicable                |                    80 |                              0 |
| Lightweight `tl 0.7.8` + `percent-encoding 2.3.2`                                        | 4/4 fixtures, 219/219 entries |                66,004 |                         65,924 |
| Full DOM `scraper 0.29.0` (default features disabled, `html5ever 0.40.1`) + same decoder | 4/4 fixtures, 219/219 entries |               619,134 |                        619,054 |

Toolchain: `rustc 1.101.0-nightly (249c1e4e3 2026-11-05)`, host
`x86_64-pc-windows-msvc`, target `wasm32-unknown-unknown`. The same crate used
`[lib] crate-type = ["cdylib", "rlib"]`, retaining an
`#[unsafe(no_mangle)] pub fn extract(body: &str) -> Vec<(String, Option<String>)>`
export to prevent removal of parser code. The three builds changed only parser
features. Release settings were `opt-level = "z"`, `lto = true`,
`codegen-units = 1`, `panic = "abort"`, and `strip = true`. Sizes are raw wasm
lengths without wasm-opt or gzip. This measures a minimal extraction prototype's
increment, not the complete manifest wasm budget.

Commands ran sequentially, never as parallel builds, always with `-j 2`:

```powershell
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release
cargo run -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --features light
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release --features light
cargo run -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --features dom
cargo build -j 2 --manifest-path "$env:TEMP/manifest-a4-proto/Cargo.toml" --lib --target wasm32-unknown-unknown --release --features dom
```

`tl` was selected: identical results on these real/synthetic inputs with 553,130
fewer incremental bytes than full DOM. It does not provide full HTML5 error
recovery; the contract targets GitHub `li` asset entries rather than browser-level
page repair. The actual implementation also validates algorithms/digest lengths,
duplicate conflicts, and stable errors; the prototype measured extraction only.
