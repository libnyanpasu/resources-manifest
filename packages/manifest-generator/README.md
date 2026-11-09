# @manifest/generator

Generates the v2 resource manifest for the cores clash-nyanpasu ships: Mihomo,
Meow and Clash Rust. Compilation runs in the wasm core through
`@manifest/runtime`; run `pnpm run build:wasm` first.

```sh
GITHUB_TOKEN=... pnpm --filter @manifest/generator run start --out ./manifest
```

The package command is `manifest-generate`. `--out` is required and receives
`resources.json` and `snapshot.json`. `GITHUB_TOKEN` is optional; it raises the
GitHub REST API rate limit and is sent only to `https://api.github.com`. GitHub
IDs are read from the response text, so the snapshot keeps every digit.

## What it produces

[`template.ts`](template.ts) is the authored template (design §9.4): one build
per platform that runs on clash-nyanpasu's minimum systems, a `stable` variant
with the newest eight releases as static history, and an `alpha` variant
resolved remotely. The template declares where releases come from; it carries
no history, fallback or checksum.

A run:

1. captures every release source from the GitHub repository named by the
   resource's `source.owner` and `source.repo`: the newest non-draft,
   non-prerelease releases for a history source, the tagged release for a tag
   source, each with all its assets and digests;
2. compiles a provisional manifest and resolves every alpha artifact's version
   through its own remote version plan, exactly as a host does with
   `finishVersion`;
3. reads each tag release again and compiles the final manifest with the
   observed versions as fallbacks.

`resources.json` is the compiled manifest and `snapshot.json` the captured
releases it was compiled from; compiling the template with that snapshot and
the manifest's `updated_at` reproduces the manifest byte for byte. Neither file
is written when the new manifest equals the existing `resources.json` apart from
`updated_at` and the existing `snapshot.json` still reproduces it. Each file is
replaced through a temporary file and a rename, the snapshot first. A missing
or malformed existing file, or a snapshot that no longer reproduces the
manifest, makes the next run rewrite both.

The generator does not write the v1 `version.json`; clash-nyanpasu's upstream
scripts keep producing it until they are retired.

## Failures

The run fails and writes nothing when:

- the artifacts of one tag source report different versions, or the tag
  release changes between the two reads, which means upstream is publishing;
  the next scheduled run retries;
- compilation fails, for example because the newest release lacks an
  artifact; every underlying `compile.*`, `validate.*` or render error is
  printed with its code and path;
- the latest release of a stable artifact has no digest, which would leave the
  default selection with checksum kind `none`;
- a request fails or exceeds its limit. All requests live in
  [`http.ts`](http.ts), with a 30-second timeout covering the body, a 2 MiB limit
  for version responses and 16 MiB for GitHub API pages.

Errors are printed as `error: <code>: <message>` with a nonzero exit status.
