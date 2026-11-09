<script lang="ts">
  import { onMount } from "svelte";
  import type { ResourceManifest } from "@manifest/metadata";
  import { initRuntime, type ValidatedManifest } from "@manifest/runtime/web";
  import {
    artifactRows,
    loadManifest,
    loadSequence,
    type ErrorView,
    type Loaded,
  } from "../model.ts";
  import Loader from "./Loader.svelte";
  import Overview from "./Overview.svelte";
  import Registries from "./Registries.svelte";
  import SelectionPanel from "./SelectionPanel.svelte";

  type Tab = "artifacts" | "selection" | "registries";

  let ready = $state(false);
  let busy = $state(false);
  let fatal = $state<string | null>(null);
  let source = $state<string | null>(null);
  let errors = $state<ErrorView[]>([]);
  let loaded = $state.raw<{
    validated: ValidatedManifest;
    manifest: ResourceManifest;
  } | null>(null);
  let tab = $state<Tab>("artifacts");
  let focus = $state<string | null>(null);

  const rows = $derived(loaded ? artifactRows(loaded.manifest) : []);

  onMount(() => {
    initRuntime().then(
      () => {
        ready = true;
        const url = new URLSearchParams(location.search).get("url");
        if (url) void openUrl(url);
      },
      (error: unknown) => {
        fatal = `Cannot load the manifest core: ${String(error)}`;
      },
    );
  });

  const startLoad = loadSequence();

  /** Replaces the page's manifest, or its errors, with a load result. */
  function apply(from: string, result: Loaded): void {
    loaded?.validated.free();
    source = from;
    focus = null;
    if (result.ok) {
      loaded = { validated: result.validated, manifest: result.manifest };
      errors = [];
    } else {
      loaded = null;
      errors = result.errors;
    }
  }

  /** Keeps `?url=` in step with the shown manifest so the page can be shared. */
  function rememberUrl(url: string | null): void {
    const location = new URL(window.location.href);
    if (url) location.searchParams.set("url", url);
    else location.searchParams.delete("url");
    history.replaceState(null, "", location);
  }

  function openText(text: string, from: string): void {
    startLoad();
    busy = false;
    apply(from, loadManifest(text));
    rememberUrl(null);
  }

  async function openFile(file: File): Promise<void> {
    const ticket = startLoad();
    busy = false;
    const text = await file.text();
    if (!ticket.isCurrent()) return;
    apply(file.name, loadManifest(text));
    rememberUrl(null);
  }

  async function openUrl(url: string): Promise<void> {
    const ticket = startLoad();
    busy = true;
    let result: Loaded;
    try {
      const response = await fetch(url, { signal: ticket.signal });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status} ${response.statusText}`);
      }
      const text = await response.text();
      result = loadManifest(text);
    } catch (error) {
      result = {
        ok: false,
        errors: [
          {
            code: "viewer.fetch_failed",
            path: "",
            message: `${error instanceof Error ? error.message : String(error)}. The server must allow cross-origin requests (CORS).`,
          },
        ],
      };
    }
    // A newer load owns the page; drop this result, freeing a loaded handle.
    if (!ticket.isCurrent()) {
      if (result.ok) result.validated.free();
      return;
    }
    busy = false;
    apply(url, result);
    rememberUrl(url);
  }

  function reveal(key: string): void {
    focus = key;
    tab = "artifacts";
  }
</script>

<header class="bar">
  <h1>Resource manifest viewer</h1>
  {#if loaded}
    <p class="meta">
      Manifest {loaded.manifest.manifest_version}
      {#if loaded.manifest.updated_at}· updated {loaded.manifest
          .updated_at}{/if}
      · {Object.keys(loaded.manifest.resources).length} resources · {rows.length}
      artifacts
      {#if loaded.validated.newerMinor}
        · <span class="warn">newer minor than this viewer</span>
      {/if}
    </p>
  {/if}
</header>

<main>
  {#if fatal}
    <p class="error-text">{fatal}</p>
  {:else if !ready}
    <p class="muted">Loading the manifest core…</p>
  {:else}
    <Loader
      {busy}
      {source}
      onurl={openUrl}
      onfile={openFile}
      ontext={openText}
    />

    {#if errors.length > 0}
      <section class="panel errors" aria-live="polite">
        <h2>
          {errors.length === 1
            ? "The manifest cannot be used"
            : `${errors.length} problems`}
        </h2>
        <ul>
          {#each errors as error, index (index)}
            <li>
              <code>{error.code}</code>
              {#if error.path}at <code>{error.path}</code>{/if}
              <div class="muted">{error.message}</div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if loaded}
      <nav class="tabs" aria-label="Views">
        <button
          class:active={tab === "artifacts"}
          onclick={() => (tab = "artifacts")}>Artifacts</button
        >
        <button
          class:active={tab === "selection"}
          onclick={() => (tab = "selection")}>Host selection</button
        >
        <button
          class:active={tab === "registries"}
          onclick={() => (tab = "registries")}>Registries and mirrors</button
        >
      </nav>
      {#if tab === "artifacts"}
        <Overview
          {rows}
          validated={loaded.validated}
          manifest={loaded.manifest}
          {focus}
        />
      {:else if tab === "selection"}
        {#if Object.keys(loaded.manifest.resources).length === 0}
          <p class="panel muted">This manifest has no resources to select.</p>
        {:else}
          <!-- A new manifest starts the selection over. -->
          {#key loaded.manifest}
            <SelectionPanel
              validated={loaded.validated}
              manifest={loaded.manifest}
              onreveal={reveal}
            />
          {/key}
        {/if}
      {:else}
        <Registries manifest={loaded.manifest} />
      {/if}
    {/if}
  {/if}
</main>
