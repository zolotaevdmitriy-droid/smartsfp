import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  build: {
    // Output goes to web-ui/dist/. The Go agent embeds this directory.
    outDir: "dist",
    emptyOutDir: true,
    // No source maps in production — keep the embed small.
    sourcemap: false,
    // Inline small assets so we don't pollute dist/assets too much.
    assetsInlineLimit: 4096,
    target: "es2020",
  },
  server: {
    port: 5173,
    // During `npm run dev`, proxy API calls to a running agent
    // (assumed to be tunnelled to localhost:9100 by scripts/open_agent_ui.py).
    proxy: {
      "/api": "http://localhost:9100",
      "/healthz": "http://localhost:9100",
      "/readyz": "http://localhost:9100",
      "/metrics": "http://localhost:9100",
    },
  },
});
