<script lang="ts">
  import { onMount } from "svelte";
  import { route, startStatusPolling, installRouter } from "./lib/stores";
  import Sidebar from "./components/Sidebar.svelte";
  import TopBar from "./components/TopBar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import Dashboard from "./pages/Dashboard.svelte";
  import Keys from "./pages/Keys.svelte";
  import Logs from "./pages/Logs.svelte";
  import Settings from "./pages/Settings.svelte";

  onMount(() => {
    const stopRouter = installRouter();
    const stopPoll = startStatusPolling(2000);
    return () => { stopRouter(); stopPoll(); };
  });
</script>

<div class="flex h-screen">
  <Sidebar />

  <main class="flex flex-1 flex-col overflow-hidden">
    <TopBar />
    <div class="flex-1 overflow-y-auto bg-slate-950">
      <div class="mx-auto max-w-6xl px-6 py-6">
        {#if $route === "dashboard"}<Dashboard />
        {:else if $route === "keys"}<Keys />
        {:else if $route === "logs"}<Logs />
        {:else if $route === "settings"}<Settings />
        {:else}<Dashboard />{/if}
      </div>
    </div>
  </main>
</div>

<Toasts />
