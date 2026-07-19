# Resource manifest v2

This document describes the variant-aware resource manifest generated at
`manifest/resources.json`. The existing `manifest/version.json` remains in place
while updater consumers migrate.

## Design goals

1. A resource key identifies a **core family**, not a release channel. Stable,
   alpha and future channels are variants of the same resource and are selected
   through tags.
2. A variant is identified semantically by its **exact tag combination**.
   `variant.id` is only a stable, human-readable alias.
3. Every compiled variant contains expanded tag objects, so a UI can render the
   channel, CPU level and toolchain without joining another catalog.
4. Every artifact declares how its version is resolved:
   - `static`: the manifest contains a bounded, traceable release history;
   - `remote`: the manifest tells the backend which URLs to request and how to
     parse the latest version and checksum.
5. Checksums are serialized as `<algorithm>:<hex>`, for example
   `sha512:0123...`.
6. Canonical artifact/metadata location (`registry`) is separate from URL
   acceleration or alternative delivery (`mirror`). The same mirror pipeline is
   used for downloads, remote version files and remote checksum metadata.
7. Registries and mirrors are named templates and may also be injected at
   runtime for user-defined download sources.

## Resource identity and channel variants

The v1 manifest exposes separate keys such as `mihomo` and `mihomo_alpha`. The
v2 resource manifest merges them:

```json
{
  "resources": {
    "mihomo": {},
    "clash_rs": {},
    "clash_premium": {}
  }
}
```

A Mihomo stable variant and alpha variant differ by their tag combination and
channel-specific source variables:

```ts
{
  id: "stable-goamd64-v2",
  name: "Stable · GOAMD64 v2",
  tags: [
    "channel:stable",
    "toolchain:go-current",
    "cpu:goamd64-v2",
  ],
  source: { release: "{version}" },
  artifacts: { /* ... */ },
}

{
  id: "alpha-goamd64-v2",
  name: "Alpha · GOAMD64 v2",
  tags: [
    "channel:alpha",
    "toolchain:go-current",
    "cpu:goamd64-v2",
  ],
  source: { release: "Prerelease-Alpha" },
  artifacts: { /* ... */ },
}
```

The compiler expands each tag and adds an order-independent `tag_key`:

```json
{
  "id": "alpha-goamd64-v2",
  "tag_key": "channel:alpha+cpu:goamd64-v2+toolchain:go-current",
  "name": "Alpha · GOAMD64 v2",
  "tags": [
    { "id": "channel:alpha", "name": "Alpha", "group": "channel" },
    {
      "id": "toolchain:go-current",
      "name": "Current Go",
      "group": "toolchain"
    },
    { "id": "cpu:goamd64-v2", "name": "GOAMD64 v2", "group": "cpu" }
  ]
}
```

A UI should render `variant.tags` directly. Selecting by exact tags remains
order-independent.

## Static version history

A stable artifact can carry several releases and their artifact-specific
checksums:

```json
{
  "artifact": "mihomo-windows-amd64-v2-{version}.zip",
  "version": {
    "type": "static",
    "latest": "v1.19.28",
    "releases": [
      {
        "version": "v1.19.28",
        "checksum": "sha256:0123456789abcdef...",
        "published_at": "2026-07-08T00:34:23Z"
      },
      {
        "version": "v1.19.27",
        "checksum": "sha256:fedcba9876543210...",
        "published_at": "2026-06-06T07:56:33Z"
      }
    ]
  }
}
```

The current generator requests a bounded set of non-prerelease GitHub releases,
paginates both releases and their assets, and copies GitHub's asset `digest`
into the static history. The default trace depth is eight releases and can be
set from 1 to 50 with `NYANPASU_RESOURCE_HISTORY_LIMIT`. `GITHUB_TOKEN` is used
when available. An asset may still appear without `checksum` when the upstream
registry does not expose a digest.

The resolver only accepts a requested static version when that version is
present in the artifact's `releases` list. This prevents callers from rendering
an arbitrary historical URL and then accidentally treating an unrelated digest
as authoritative.

Checksums use a single serialized representation:

```text
md5:<32 hex characters>
sha256:<64 hex characters>
sha512:<128 hex characters>
```

The compiler normalizes the algorithm and hexadecimal value to lowercase.

## Remote version and checksum resources

Mutable prerelease tags often retain only the newest assets and cannot provide a
stable historical chain. Such an artifact uses a declarative remote resolution
plan instead of pretending to have history:

```json
{
  "artifact": "mihomo-windows-amd64-v2-{version}.zip",
  "version": {
    "type": "remote",
    "fallback": {
      "version": "alpha-e911985"
    },
    "version": {
      "resource": {
        "registry": "github-release",
        "artifact": "version.txt"
      },
      "parser": {
        "type": "text",
        "trim": true
      }
    },
    "checksum": {
      "resource": {
        "registry": "github-release-metadata"
      },
      "parser": {
        "type": "github-release-assets"
      }
    }
  }
}
```

For the Mihomo alpha variant, variant-level `source.release` is
`Prerelease-Alpha`. The two remote requests therefore resolve to:

```text
https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt
https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha
```

The first response supplies the latest mutable version. The second response
contains GitHub's artifact digest; the parser selects the digest adjacent to the
rendered artifact name.

`fallback` is the snapshot observed when the manifest was generated. When the
GitHub release API exposes the current asset digest, the generator also embeds
that artifact-specific checksum in the fallback. It is still not authoritative
latest state. The resolver exposes it as `fallback_checksum`, never as the
normal authoritative `checksum`, while also returning `checksum_remote`.
`resolveResourceDownload` deliberately requires an explicit version for a remote
artifact, so a consumer cannot silently skip the remote version request. A
backend may explicitly use the fallback only after applying its own
offline/failure policy.

### Supported remote version parsers

- `text`: use the response body, optionally trimming whitespace;
- `regex`: apply a regular expression and return a configured capture group.

### Supported remote checksum parsers

- `checksum`: the complete body is `<algorithm>:<hex>`;
- `gnu`: find `<hex>  <artifact>`, with the algorithm declared in the manifest;
- `bsd`: find `ALGORITHM (<artifact>) = <hex>`;
- `github-release-assets`: find the selected artifact and its adjacent `md5:`,
  `sha256:` or `sha512:` digest in GitHub release-assets metadata.

These parser names are protocol between the manifest generator and backend. A
Rust consumer should implement the same deterministic semantics rather than
running arbitrary scripts from the manifest.

## Backend resolution flow

For a selected resource, platform and tag combination:

1. Select the variant by `variant.id`, exact tags, or the platform default.
2. Call the equivalent of `resolveResourceVersion`.
3. For a static artifact, choose `latest` or another entry from `releases`.
4. For a remote artifact, request the returned mirror-aware version URL and
   parse the response. Pass the resulting version explicitly to artifact
   resolution.
5. Render the artifact URL.
6. For a static artifact, use its embedded authoritative `checksum`. For a
   remote artifact, request and parse `checksum_remote` for the rendered
   artifact name. Use `fallback_checksum` only under an explicit offline policy.
7. Download and verify before decompression/replacement.

The manifest does not ask the backend to trust version text as a checksum. The
checksum always belongs to the concrete rendered artifact.

## Registry and mirror model

An artifact registry produces the canonical download URL:

```json
{
  "github-release": {
    "name": "GitHub Releases",
    "url_template": "https://github.com/{owner}/{repo}/releases/download/{release}/{artifact}"
  }
}
```

A metadata registry may omit `{artifact}`:

```json
{
  "github-release-metadata": {
    "name": "GitHub Release Assets Metadata",
    "url_template": "https://github.com/{owner}/{repo}/releases/expanded_assets/{release}"
  }
}
```

A mirror transforms either canonical URL:

```json
{
  "nyanpasu": {
    "name": "Nyanpasu GitHub mirror",
    "url_template": "https://nyanpasu-script.majokeiko.com{path_and_query}",
    "registries": ["github-release", "github-release-metadata"],
    "priority": 10
  }
}
```

Supported mirror variables include:

- `{url}` and `{url_encoded}`;
- `{scheme}`, `{host}`, `{hostname}` and `{port}`;
- `{path}`, `{pathname}`, `{query}` and `{path_and_query}`;
- registry/resource variables such as `{owner}`, `{repo}`, `{release}` and
  `{artifact}`.

The selected mirror is applied consistently to:

- the final binary artifact;
- a remote `version.txt` or equivalent resource;
- a remote checksum list or GitHub release metadata page.

`priority` is a strategy hint only; the pure resolver does not perform network
probing or speed tests.

## Runtime custom artifact sources

`resolveResourceDownload` still accepts either a named artifact registry or an
inline runtime registry/mirror. User-defined delivery configuration therefore
stays outside the generated manifest:

```ts
resolveResourceDownload(manifest, "mihomo", {
  platform: "windows-x86_64",
  variantId: "stable-goamd64-v2",
  registry: {
    name: "Private object storage",
    url_template: "https://registry.example/{bucket}/{artifact}",
  },
  source: { bucket: "stable" },
  mirror: {
    name: "Custom accelerator",
    url_template: "https://accelerator.example/fetch?url={url_encoded}",
  },
});
```

The result contains both `canonical_url` and `download_url`, allowing callers to
log the source and retry without acceleration.

## Validation invariants

The compiler rejects templates when:

- a tag or variant ID is invalid or duplicated (`+` is reserved as the tag-key
  separator);
- two variants have the same exact tag combination;
- a variant references an unknown tag;
- a variant has two tags from the same non-empty group;
- multiple variants support a platform but no platform default is configured;
- a default variant does not provide an artifact for that platform;
- a static `latest` version is absent from the artifact history;
- a history contains duplicate versions;
- a generated latest release snapshot does not contain the advertised artifact;
- a checksum does not use a supported `<algorithm>:<hex>` representation;
- a remote resource references an unknown registry;
- a remote parser is invalid;
- a registry, mirror or template variable cannot be resolved.

If only one variant supports a platform, its default is inferred.

## Migration from manifest v1

The generator writes both files:

- `manifest/version.json`: unchanged v1 schema and five legacy channel keys for
  existing updater/build code;
- `manifest/resources.json`: v2 schema with one key per core family, channel
  tags, static history and remote resolution instructions.

This makes schema rollout independent from the updater UI/backend migration.
Once every consumer resolves resources through v2, the legacy `latest`,
`arch_template` and hard-coded GitHub path mapping can be removed.

## Current integration boundary

This change implements the template compiler, release-history generator,
mirror-aware remote resolution plan and parsing reference functions. Existing
Rust updater and build consumers continue to read `manifest/version.json` until
they are migrated. A consumer migration should expose compatible variants
through IPC, persist the selected exact tag combination, implement the remote
parser protocol, and verify the resolved checksum before decompression.
