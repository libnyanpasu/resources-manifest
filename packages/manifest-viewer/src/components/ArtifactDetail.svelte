<script lang="ts">
  import type { ResourceManifest } from "@manifest/metadata";
  import type { ValidatedManifest } from "@manifest/runtime/web";
  import { artifactDetail, type ArtifactRow } from "../model.ts";

  interface Props {
    validated: ValidatedManifest;
    manifest: ResourceManifest;
    row: ArtifactRow;
  }

  let { validated, manifest, row }: Props = $props();

  const detail = $derived(artifactDetail(validated, manifest, row));
  const version = $derived(detail.artifact.version);
  const canonical = $derived(detail.versions[0]!);
</script>

<div class="detail-body">
  <p class="muted small">Path <code>{row.path}</code></p>

  <h3>Version</h3>
  {#if canonical.error}
    <p class="error-text">
      <code>{canonical.error.code}</code>
      {canonical.error.message}
    </p>
  {/if}
  {#if version.type === "static"}
    <p class="muted small">
      Static history; latest <code>{version.latest}</code>.
    </p>
    <table class="compact">
      <thead>
        <tr
          ><th>Version</th><th>Checksum</th><th>Published</th><th>Source</th
          ></tr
        >
      </thead>
      <tbody>
        {#each version.releases as release (release.version)}
          <tr class:latest={release.version === version.latest}>
            <td><code>{release.version}</code></td>
            <td><code class="wrap">{release.checksum ?? "—"}</code></td>
            <td>{release.published_at ?? "—"}</td>
            <td>
              {#each Object.entries(release.source ?? {}) as [key, value] (key)}
                <code>{key}={value}</code>
              {:else}—{/each}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <p class="muted small">
      Resolved remotely by the host{canonical.plan?.remote?.format
        ? " from several responses"
        : ""}.
    </p>
    {#if canonical.plan?.remote}
      <table class="compact">
        <thead>
          <tr><th>Part</th><th>Parser</th></tr>
        </thead>
        <tbody>
          {#each canonical.plan.remote.parts as part, index (index)}
            <tr>
              <td>{part.name ?? "—"}</td>
              <td><code class="wrap">{JSON.stringify(part.parser)}</code></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if canonical.plan.remote.format}
        <p>Format <code>{canonical.plan.remote.format}</code></p>
      {/if}
    {/if}
    <table class="compact">
      <thead>
        <tr><th>Via</th><th>Part</th><th>Request</th></tr>
      </thead>
      <tbody>
        {#each detail.versions as view, index (index)}
          {#if view.plan?.remote}
            {#each view.plan.remote.parts as part, partIndex (partIndex)}
              {@const url =
                part.request.mirror_url ?? part.request.canonical_url}
              <tr>
                <td>{view.mirror ?? "canonical"}</td>
                <td>{part.name ?? "—"}</td>
                <td>
                  <a href={url} rel="noreferrer"
                    ><code class="wrap">{url}</code></a
                  >
                </td>
              </tr>
            {/each}
          {:else if view.error}
            <tr>
              <td>{view.mirror ?? "canonical"}</td>
              <td colspan="2" class="muted">
                <code>{view.error.code}</code>
                {view.error.message}
              </td>
            </tr>
          {/if}
        {/each}
      </tbody>
    </table>
    {#if version.fallback}
      <p>
        Fallback <code>{version.fallback.version}</code>
        {#if version.fallback.checksum}
          · snapshot checksum <code class="wrap"
            >{version.fallback.checksum}</code
          >
        {/if}
        {#if version.fallback.published_at}
          · published {version.fallback.published_at}
        {/if}
      </p>
    {:else}
      <p class="muted">No fallback.</p>
    {/if}
  {/if}

  <h3>Downloads</h3>
  {#if detail.downloads.length === 0}
    <p class="muted">No version to plan a download for.</p>
  {:else}
    <table class="compact">
      <thead>
        <tr><th>Version</th><th>Via</th><th>URL</th><th>Checksum</th></tr>
      </thead>
      <tbody>
        {#each detail.downloads as download, index (index)}
          <tr>
            <td><code>{download.version}</code></td>
            <td>{download.mirror ?? "canonical"}</td>
            {#if download.plan}
              {@const url =
                download.plan.request.mirror_url ??
                download.plan.request.canonical_url}
              <td>
                <a href={url} rel="noreferrer"
                  ><code class="wrap">{url}</code></a
                >
              </td>
              <td>
                {#if download.plan.checksum}
                  <span class={`kind kind-${download.plan.checksum.kind}`}
                    >{download.plan.checksum.kind}</span
                  >
                  <code class="wrap">{download.plan.checksum.checksum}</code>
                {:else if download.plan.checksum_request}
                  <span class="kind kind-parsed">parsed</span>
                  {download.plan.checksum_request.parser.type} from
                  <code class="wrap"
                    >{download.plan.checksum_request.request.mirror_url ??
                      download.plan.checksum_request.request
                        .canonical_url}</code
                  >
                {:else}
                  <span class="kind kind-none">none</span>
                {/if}
              </td>
            {:else if download.error}
              <td colspan="2" class="muted">
                <code>{download.error.code}</code>
                {download.error.message}
              </td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <details>
    <summary>Artifact JSON</summary>
    <pre>{JSON.stringify(detail.artifact, null, 2)}</pre>
  </details>
</div>
