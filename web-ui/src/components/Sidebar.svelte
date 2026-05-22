<script lang="ts">
  import { LayoutDashboard, KeyRound, ScrollText, Settings, ShieldCheck } from "lucide-svelte";
  import { route, navigate } from "../lib/stores";

  const nav = [
    { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
    { id: "keys",      label: "Ключи",      icon: KeyRound },
    { id: "logs",      label: "Логи",       icon: ScrollText },
    { id: "settings",  label: "Настройки",  icon: Settings },
  ] as const;
</script>

<aside
  class="flex h-screen w-60 shrink-0 flex-col border-r border-slate-800
         bg-slate-900/80 backdrop-blur-md"
>
  <div class="flex items-center gap-2 px-4 py-4">
    <span
      class="grid h-8 w-8 place-items-center rounded-md
             bg-gradient-to-br from-brand-500 to-brand-700"
    >
      <ShieldCheck size={18} class="text-white" />
    </span>
    <div class="leading-tight">
      <div class="text-sm font-semibold">ACM-UZ</div>
      <div class="text-[10px] uppercase tracking-wider text-slate-500">
        Smart SFP encryptor
      </div>
    </div>
  </div>

  <nav class="mt-2 flex flex-col gap-0.5 px-2">
    {#each nav as item (item.id)}
      {@const active = $route === item.id}
      <button
        onclick={() => navigate(item.id)}
        class="group flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left
               text-sm transition-colors
               {active
                 ? 'bg-brand-600/15 text-brand-300'
                 : 'text-slate-300 hover:bg-slate-800/60 hover:text-slate-100'}"
      >
        <item.icon size={16} />
        <span class="flex-1">{item.label}</span>
        {#if active}
          <span class="h-1.5 w-1.5 rounded-full bg-brand-400"></span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="mt-auto px-4 py-3">
    <div class="text-[10px] text-slate-500">v0.1.0 · build 0</div>
  </div>
</aside>
