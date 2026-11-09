<script lang="ts">
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import type { ResourceManifest } from "@manifest/metadata";
  import type { ValidatedManifest } from "@manifest/runtime/web";
  import {
    EMPTY_QUERY,
    facets,
    filterRows,
    type ArtifactRow,
    type Query,
  } from "../model.ts";
  import ArtifactDetail from "./ArtifactDetail.svelte";

  interface Props {
    rows: ArtifactRow[];
    validated: ValidatedManifest;
    manifest: ResourceManifest;
    /** Row to expand and scroll to, set by the host selection view. */
    focus: string | null;
  }

  let { rows, validated, manifest, focus }: Props = $props();

  let query = $state<Query>({ ...EMPTY_QUERY });
  const expanded = new SvelteSet<string>();

  const options = $derived(facets(rows));
  const visible = $derived(filterRows(rows, query));

  $effect(() => {
    if (!focus) return;
    query = { ...EMPTY_QUERY };
    expanded.add(focus);
    const id = rowId(focus);
    void tick().then(() =>
      document.getElementById(id)?.scrollIntoView({ block: "center" }),
    );
  });

  function rowId(key: string): string {
    return `artifact-${key.replaceAll("/", "--")}`;
  }

  function toggle(key: string): void {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
  }
</script>

<section class="panel">
  <div class="filters">
    <input
      type="search"
      placeholder="Search resources, variants, tags, artifacts, versions"
      aria-label="Search"
      bind:value={query.text}
    />
    <select aria-label="Resource" bind:value={query.resource}>
      <option value="">All resources</option>
      {#each options.resources as resource (resource.id)}
        <option value={resource.id}>{resource.name}</option>
      {/each}
    </select>
    <select aria-label="Platform" bind:value={query.platform}>
      <option value="">All platforms</option>
      {#each options.platforms as platform (platform)}
        <option value={platform}>{platform}</option>
      {/each}
    </select>
    <select aria-label="Tag" bind:value={query.tag}>
      <option value="">All tags</option>
      {#each options.tags as tag (tag)}
        <option value={tag}>{tag}</option>
      {/each}
    </select>
    <select aria-label="Checksum kind" bind:value={query.kind}>
      <option value="">All kinds</option>
      {#each options.kinds as kind (kind)}
        <option value={kind}>{kind}</option>
      {/each}
    </select>
    <span class="muted count">{visible.length} of {rows.length}</span>
  </div>

  <div class="scroll">
    <table class="artifacts">
      <thead>
        <tr>
          <th>Resource</th>
          <th>Variant</th>
          <th>Platform</th>
          <th>Artifact</th>
          <th>Version</th>
          <th>Kind</th>
        </tr>
      </thead>
      <tbody>
        {#each visible as row (row.key)}
          {@const open = expanded.has(row.key)}
          <tr
            id={rowId(row.key)}
            class="artifact-row"
            class:open
            class:focused={row.key === focus}
            onclick={() => toggle(row.key)}
          >
            <td>
              <button
                class="disclosure"
                aria-expanded={open}
                aria-label={`${open ? "Collapse" : "Expand"} ${row.key}`}
                onclick={(event) => {
                  event.stopPropagation();
                  toggle(row.key);
                }}>{open ? "▾" : "▸"}</button
              >
              {row.resourceName}
            </td>
            <td>
              <code>{row.variantId}</code>
              {#if row.isDefault}<span class="badge">default</span>{/if}
              {#each row.tags as tag (tag)}<span class="tag">{tag}</span>{/each}
            </td>
            <td><code>{row.platform}</code></td>
            <td><code class="wrap">{row.artifact}</code></td>
            <td>
              {#if row.version}<code>{row.version}</code>{:else}—{/if}
              <div class="muted small">
                {row.versionType === "static"
                  ? `static · ${row.releases} ${row.releases === 1 ? "release" : "releases"}`
                  : "remote · fallback"}
              </div>
            </td>
            <td><span class={`kind kind-${row.kind}`}>{row.kind}</span></td>
          </tr>
          {#if open}
            <tr class="detail">
              <td colspan="6">
                <ArtifactDetail {validated} {manifest} {row} />
              </td>
            </tr>
          {/if}
        {:else}
          <tr><td colspan="6" class="muted">No artifact matches.</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>
