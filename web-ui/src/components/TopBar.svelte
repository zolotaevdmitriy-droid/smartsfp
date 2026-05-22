<script lang="ts">
  import { status, statusError } from "../lib/stores";
  import { Activity, AlertCircle } from "lucide-svelte";

  let title = $derived(routeTitle());
  function routeTitle() {
    if (typeof location === "undefined") return "Dashboard";
    const r = (location.hash || "#/dashboard").replace(/^#\//, "");
    return ({
      dashboard: "Dashboard",
      keys: "Ключи",
      logs: "Логи",
      settings: "Настройки",
    } as Record<string, string>)[r] || "Dashboard";
  }
</script>

<header
  class="flex h-14 items-center justify-between border-b border-slate-800
         bg-slate-900/40 px-6 backdrop-blur-md"
>
  <h1 class="text-lg font-semibold text-slate-100">{title}</h1>

  <div class="flex items-center gap-3">
    {#if $statusError}
      <span
        class="flex items-center gap-1.5 rounded-full border border-rose-500/40
               bg-rose-500/10 px-3 py-1 text-xs text-rose-300"
      >
        <AlertCircle size={12} />
        Cryptod недоступен
      </span>
    {:else if $status?.running}
      <span
        class="flex items-center gap-1.5 rounded-full border border-emerald-500/40
               bg-emerald-500/10 px-3 py-1 text-xs text-emerald-300"
      >
        <span
          class="relative inline-flex h-2 w-2 rounded-full bg-emerald-400"
        >
          <span
            class="absolute inset-0 animate-ping rounded-full bg-emerald-400 opacity-75"
          ></span>
        </span>
        UP
      </span>
    {:else}
      <span
        class="flex items-center gap-1.5 rounded-full border border-slate-700
               bg-slate-800 px-3 py-1 text-xs text-slate-400"
      >
        <Activity size={12} />
        …
      </span>
    {/if}

    <span class="text-xs text-slate-500">
      smart-sfp@192.168.0.99
    </span>
  </div>
</header>
