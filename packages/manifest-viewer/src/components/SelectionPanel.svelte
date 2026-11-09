<script lang="ts">
  import type { ResourceManifest, SupportedPlatform } from "@manifest/metadata";
  import type { ValidatedManifest } from "@manifest/runtime/web";
  import { simulate } from "../model.ts";

  interface Props {
    validated: ValidatedManifest;
    manifest: ResourceManifest;
    onreveal: (key: string) => void;
  }

  let { validated, manifest, onreveal }: Props = $props();

  const resourceIds = $derived(Object.keys(manifest.resources));
  let resourceId = $state("");
  let platform = $state("");
  let mode = $state<"default" | "tags" | "id">("default");
  let tags = $state<string[]>([]);
  let variantId = $state("");

  const chosenResource = $derived(
    resourceIds.includes(resourceId) ? resourceId : resourceIds[0]!,
  );
  const resource = $derived(manifest.resources[chosenResource]!);
  const platforms = $derived(
    [
      ...new Set(
        resource.variants.flatMap((variant) => Object.keys(variant.artifacts)),
      ),
    ].sort(),
  );
  const chosenPlatform = $derived(
    platforms.includes(platform) ? platform : (platforms[0] ?? ""),
  );
  const result = $derived(
    simulate(
      validated,
      chosenResource,
      chosenPlatform as SupportedPlatform,
      mode === "tags"
        ? { tags }
        : mode === "id"
          ? { id: variantId || null }
          : {},
    ),
  );
</script>

<section class="panel selection">
  <p class="muted">
    Selects the artifact as a host would with <code>selectVariant</code>: the
    platform default, an exact tag set, or a variant ID.
  </p>
  <div class="filters">
    <label>
      Resource
      <select
        value={chosenResource}
        onchange={(event) => {
          // Tags and variant IDs belong to one resource.
          resourceId = event.currentTarget.value;
          tags = [];
          variantId = "";
        }}
      >
        {#each resourceIds as id (id)}
          <option value={id}>{manifest.resources[id]!.name}</option>
        {/each}
      </select>
    </label>
    <label>
      Platform
      <select
        value={chosenPlatform}
        onchange={(event) => (platform = event.currentTarget.value)}
      >
        {#each platforms as value (value)}
          <option {value}>{value}</option>
        {/each}
      </select>
    </label>
    <label>
      Select by
      <select bind:value={mode}>
        <option value="default">platform default</option>
        <option value="tags">tags</option>
        <option value="id">variant ID</option>
      </select>
    </label>
  </div>

  {#if mode === "tags"}
    <fieldset>
      <legend>Tags</legend>
      {#each resource.tags as tag (tag.id)}
        <label class="check">
          <input type="checkbox" value={tag.id} bind:group={tags} />
          <code>{tag.id}</code>
          {tag.name}{#if tag.group}<span class="muted">
              · {tag.group}</span
            >{/if}
        </label>
      {/each}
    </fieldset>
  {:else if mode === "id"}
    <label>
      Variant ID
      <select bind:value={variantId}>
        <option value="">(none)</option>
        {#each resource.variants as variant (variant.id)}
          <option value={variant.id}>{variant.id}</option>
        {/each}
      </select>
    </label>
  {/if}

  <div class="result" aria-live="polite">
    {#if result.ok}
      Selected variant <code>{result.variantId}</code>
      <button type="button" onclick={() => onreveal(result.key)}
        >Show artifact</button
      >
    {:else}
      <span class="error-text"><code>{result.error.code}</code></span>
      {result.error.message}
    {/if}
  </div>
</section>
