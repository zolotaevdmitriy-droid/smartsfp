<script lang="ts">
  import type { Snippet } from "svelte";
  type Props = {
    title?: string;
    subtitle?: string;
    children: Snippet;
    actions?: Snippet;
    class?: string;
  };
  let { title, subtitle, children, actions, class: extra = "" }: Props = $props();
</script>

<section
  class="rounded-lg border border-slate-800 bg-slate-900/70 shadow-sm
         backdrop-blur-sm {extra}"
>
  {#if title || actions}
    <header
      class="flex items-center justify-between gap-4 border-b border-slate-800
             px-5 py-3"
    >
      <div>
        {#if title}
          <h2 class="text-base font-semibold text-slate-100">{title}</h2>
        {/if}
        {#if subtitle}
          <p class="mt-0.5 text-xs text-slate-400">{subtitle}</p>
        {/if}
      </div>
      {#if actions}
        <div class="flex items-center gap-2">{@render actions()}</div>
      {/if}
    </header>
  {/if}
  <div class="px-5 py-4">{@render children()}</div>
</section>
