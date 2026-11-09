# @manifest/viewer

A static page for inspecting a compiled resource manifest in the browser. It
runs `manifest-core` through the browser entry of `@manifest/runtime`, so
validation, checksum kinds, selection and plans are the ones hosts compute.
Built with Astro and one Svelte island; deployed to GitHub Pages at
<https://libnyanpasu.github.io/resources-manifest/>.

```sh
pnpm run build:viewer                       # builds wasm, then dist/
pnpm --filter @manifest/viewer run dev      # local development server
pnpm --filter @manifest/viewer run check    # svelte-check
```

## Loading a manifest

Load `resources.json` by URL, by opening or dropping a file, or by pasting
JSON. A URL load is kept in the `?url=` query parameter, so the page can be
shared with the manifest preselected. The server must allow cross-origin
requests; `raw.githubusercontent.com` does. Malformed JSON and every
`validate_manifest` error are listed with their code and path.

## Views

- **Artifacts:** one row per resource, variant and platform with the artifact,
  version and checksum kind, as in the generator's `summary.md`. Rows can be
  searched (every word must match) and filtered by resource, platform, tag and
  kind. Expanding a row shows its locator, the static history or the remote
  version requests with parsers and format, the fallback, the download URL and
  checksum source for every version through the canonical URL and each mirror,
  and the artifact JSON.
- **Host selection:** `selectVariant` for a resource and platform by default,
  exact tag set or variant ID, including its rejection codes; the selected
  artifact can be opened in the artifacts view.
- **Registries and mirrors:** URL templates, variables, mirror priorities and
  the registries each mirror serves.

The page does not request version or checksum responses itself: release
downloads on github.com do not allow cross-origin reads.

## Deployment

[`.github/workflows/pages.yml`](../../.github/workflows/pages.yml) builds the
page on pushes to `main` that touch the viewer, the runtime, the metadata
package or the Rust crates, and on manual dispatch. It installs Rust 1.95.0
with the wasm target and the wasm-bindgen CLI version locked in `Cargo.lock`,
runs `pnpm run build:viewer`, and deploys `dist/` to GitHub Pages. The
repository's Pages source must be set to GitHub Actions.

## Layout and checks

`src/model.ts` computes everything the components show and is tested with
plain values in `model_test.ts`, which `pnpm run test` runs. Components under
`src/components/` only render it. The root TypeScript check excludes this
package; `pnpm run typecheck` runs its `svelte-check` with the package's
`tsconfig.json`, which uses TypeScript 6 because `svelte-check` needs the
TypeScript compiler API.
