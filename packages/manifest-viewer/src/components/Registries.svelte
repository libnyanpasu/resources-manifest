<script lang="ts">
  import type { ResourceManifest } from "@manifest/metadata";

  interface Props {
    manifest: ResourceManifest;
  }

  let { manifest }: Props = $props();

  const mirrors = $derived(
    Object.entries(manifest.mirrors).sort(
      ([, a], [, b]) => (a.priority ?? 0) - (b.priority ?? 0),
    ),
  );
</script>

<section class="panel">
  <h2>Registries</h2>
  <div class="scroll">
    <table class="compact">
      <thead>
        <tr><th>ID</th><th>Name</th><th>URL template</th><th>Variables</th></tr>
      </thead>
      <tbody>
        {#each Object.entries(manifest.registries) as [id, registry] (id)}
          <tr>
            <td><code>{id}</code></td>
            <td>{registry.name}</td>
            <td><code class="wrap">{registry.url_template}</code></td>
            <td>
              {#each Object.entries(registry.variables ?? {}) as [key, value] (key)}
                <code>{key}={value}</code>
              {:else}—{/each}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <h2>Mirrors</h2>
  {#if mirrors.length === 0}
    <p class="muted">No mirrors.</p>
  {:else}
    <div class="scroll">
      <table class="compact">
        <thead>
          <tr>
            <th>ID</th><th>Name</th><th>Priority</th><th>URL template</th><th
              >Registries</th
            >
          </tr>
        </thead>
        <tbody>
          {#each mirrors as [id, mirror] (id)}
            <tr>
              <td><code>{id}</code></td>
              <td>{mirror.name}</td>
              <td>{mirror.priority ?? 0}</td>
              <td><code class="wrap">{mirror.url_template}</code></td>
              <td>
                {#each mirror.registries ?? [] as registry (registry)}
                  <code>{registry}</code>
                {:else}all{/each}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
