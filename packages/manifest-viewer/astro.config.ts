import svelte from "@astrojs/svelte";
import { defineConfig } from "astro/config";

// Deployed as the project site of libnyanpasu/resources-manifest on GitHub Pages.
export default defineConfig({
  site: "https://libnyanpasu.github.io",
  base: "/resources-manifest",
  integrations: [svelte()],
});
