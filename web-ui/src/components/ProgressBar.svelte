<script lang="ts">
  type Props = {
    pct: number;          // 0..100
    label?: string;       // text on the left
    value?: string;       // text on the right (defaults to pct%)
    tone?: "ok" | "warn" | "err" | "auto";
  };
  let { pct, label, value, tone = "auto" }: Props = $props();

  const clamped = $derived(Math.max(0, Math.min(100, pct)));
  const effective = $derived(
    tone === "auto"
      ? clamped < 70 ? "ok" : clamped < 90 ? "warn" : "err"
      : tone
  );
  const fillCls = $derived({
    ok:   "bg-emerald-500/70",
    warn: "bg-amber-500/80",
    err:  "bg-rose-500/80",
  }[effective]);
</script>

<div class="space-y-1">
  {#if label || value}
    <div class="flex items-center justify-between text-xs">
      {#if label}<span class="text-slate-400">{label}</span>{/if}
      <span class="font-mono text-slate-300">
        {value ?? clamped.toFixed(0) + "%"}
      </span>
    </div>
  {/if}
  <div class="h-1.5 overflow-hidden rounded-full bg-slate-800">
    <div
      class="h-full rounded-full transition-all duration-300 {fillCls}"
      style="width: {clamped}%"
    ></div>
  </div>
</div>
