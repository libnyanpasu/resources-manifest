# GitHub REST API fixtures (D4b)

Real response bodies saved with
`curl -fLsS --max-time 60 -H "User-Agent: manifest-conformance" <URL> -o <file>`,
unauthenticated, without formatting or trimming. They use the `.txt` extension so
neither Prettier nor the corpus loaders treat them as JSON documents. Capture
times are UTC download completion times.

| File                           | Source URL                                                    | Captured at (UTC)    |
| ------------------------------ | ------------------------------------------------------------- | -------------------- |
| `clash-rs-releases-latest.txt` | https://api.github.com/repos/ibigbug/clash-rs/releases/latest | 2026-11-09T09:39:24Z |
| `clash-rs-commits-latest.txt`  | https://api.github.com/repos/ibigbug/clash-rs/commits/latest  | 2026-11-09T09:39:26Z |

At capture time the latest stable release was `v0.10.11` and the rolling
`latest` tag pointed at commit `bd22442ad6a333e362e3aa669cbb6fb4eda3fa64`, so the
composite Clash Rust alpha version (design §9.2) is `0.10.11-alpha+sha.bd22442`.
`kind.plan.composite_version` checks that value in Rust and Node.
