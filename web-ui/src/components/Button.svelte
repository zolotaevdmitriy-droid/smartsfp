<script lang="ts">
  import type { Snippet } from "svelte";
  type Props = {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    disabled?: boolean;
    type?: "button" | "submit" | "reset";
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  };
  let {
    variant = "primary",
    size = "md",
    disabled = false,
    type = "button",
    onclick,
    children,
  }: Props = $props();

  const variantCls = {
    primary:
      "bg-brand-600 text-white hover:bg-brand-500 active:bg-brand-700 " +
      "shadow-sm shadow-brand-900/30",
    secondary:
      "bg-slate-800 text-slate-100 hover:bg-slate-700 border border-slate-700",
    ghost:
      "bg-transparent text-slate-300 hover:bg-slate-800 hover:text-slate-100",
    danger:
      "bg-rose-600 text-white hover:bg-rose-500 active:bg-rose-700",
  }[variant];
  const sizeCls = size === "sm" ? "px-2.5 py-1 text-xs" : "px-3.5 py-1.5 text-sm";
</script>

<button
  {type}
  {disabled}
  {onclick}
  class="inline-flex items-center justify-center gap-1.5 rounded-md font-medium
         transition-colors disabled:cursor-not-allowed disabled:opacity-50
         {variantCls} {sizeCls}"
>
  {@render children()}
</button>
