<script lang="ts">
  interface Props {
    busy: boolean;
    source: string | null;
    onurl: (url: string) => void;
    onfile: (file: File) => void;
    ontext: (text: string, source: string) => void;
  }

  let { busy, source, onurl, onfile, ontext }: Props = $props();

  let url = $state("");
  let pasted = $state("");
  let dragging = $state(false);

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    if (url.trim()) onurl(url.trim());
  }

  function drop(event: DragEvent): void {
    event.preventDefault();
    dragging = false;
    const file = event.dataTransfer?.files[0];
    if (file) onfile(file);
  }
</script>

<section
  class="panel loader"
  class:dragging
  aria-label="Load a manifest"
  ondragover={(event) => {
    event.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={drop}
>
  <form onsubmit={submit}>
    <label for="manifest-url">Manifest URL</label>
    <div class="row">
      <input
        id="manifest-url"
        type="url"
        placeholder="https://…/resources.json"
        bind:value={url}
      />
      <button type="submit" disabled={busy || !url.trim()}>
        {busy ? "Loading…" : "Load"}
      </button>
    </div>
  </form>
  <div class="row secondary">
    <!-- The input stays focusable; the label is its visible control. -->
    <label class="file">
      <input
        class="visually-hidden"
        type="file"
        accept=".json,application/json"
        onchange={(event) => {
          const file = event.currentTarget.files?.[0];
          if (file) onfile(file);
          event.currentTarget.value = "";
        }}
      />
      Open a file
    </label>
    <span class="muted">or drop resources.json here</span>
  </div>
  <details>
    <summary>Paste JSON</summary>
    <textarea
      aria-label="Manifest JSON"
      rows="6"
      bind:value={pasted}
      spellcheck="false"></textarea>
    <button
      type="button"
      disabled={!pasted.trim()}
      onclick={() => ontext(pasted, "pasted JSON")}>Load pasted JSON</button
    >
  </details>
  {#if source}
    <p class="muted source">Source: <code>{source}</code></p>
  {/if}
</section>
