<script lang="ts">
  import type { Component } from "svelte";
  type Props = {
    label: string;
    value: string | number;
    sub?: string;
    icon?: Component;
    tone?: "neutral" | "ok" | "warn" | "err";
  };
  let { label, value, sub, icon: Icon, tone = "neutral" }: Props = $props();

  const toneClass: Record<NonNullable<Props["tone"]>, string> = {
    neutral: "text-slate-100",
    ok:      "text-emerald-400",
    warn:    "text-amber-400",
    err:     "text-rose-400",
  };
  const ringClass: Record<NonNullable<Props["tone"]>, string> = {
    neutral: "bg-slate-800/60 text-slate-300",
    ok:      "bg-emerald-500/10 text-emerald-400",
    warn:    "bg-amber-500/10 text-amber-400",
    err:     "bg-rose-500/10 text-rose-400",
  };
</script>

<div
  class="rounded-lg border border-slate-800 bg-slate-900/70 p-4
         transition-colors hover:bg-slate-900"
>
  <div class="flex items-center justify-between">
    <span class="text-xs uppercase tracking-wide text-slate-400">{label}</span>
    {#if Icon}
      <span class="grid h-7 w-7 place-items-center rounded {ringClass[tone]}">
        <Icon size={14} />
      </span>
    {/if}
  </div>
  <div class="mt-2 font-mono text-2xl font-semibold {toneClass[tone]}">
    {value}
  </div>
  {#if sub}
    <div class="mt-0.5 font-mono text-xs text-slate-500">{sub}</div>
  {/if}
</div>
