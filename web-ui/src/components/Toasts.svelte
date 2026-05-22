<script lang="ts">
  import { toasts } from "../lib/stores";
  import { CheckCircle2, AlertTriangle, AlertCircle, X } from "lucide-svelte";
</script>

<div class="fixed top-4 right-4 z-50 flex w-80 flex-col gap-2">
  {#each $toasts as t (t.id)}
    <div
      class="flex items-start gap-3 rounded-lg border px-3 py-2.5 shadow-lg backdrop-blur-md
             {t.level === 'ok'   ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-100' : ''}
             {t.level === 'warn' ? 'border-amber-500/30 bg-amber-500/10 text-amber-100'  : ''}
             {t.level === 'err'  ? 'border-rose-500/30 bg-rose-500/10 text-rose-100'    : ''}"
    >
      <span class="mt-0.5">
        {#if t.level === "ok"}<CheckCircle2 size={16} />
        {:else if t.level === "warn"}<AlertTriangle size={16} />
        {:else}<AlertCircle size={16} />{/if}
      </span>
      <span class="flex-1 text-sm">{t.text}</span>
      <button
        class="text-slate-400 hover:text-slate-200"
        aria-label="dismiss"
        onclick={() => toasts.update((a) => a.filter((x) => x.id !== t.id))}
      >
        <X size={14} />
      </button>
    </div>
  {/each}
</div>
