<script lang="ts">
  import { api, ALGORITHMS, algorithmById } from "../lib/api";
  import { status, toast } from "../lib/stores";
  import Card from "../components/Card.svelte";
  import Button from "../components/Button.svelte";
  import { Shuffle, RotateCw, KeyRound, Trash2 } from "lucide-svelte";

  type Hist = { ts: number; key_id: number; algo: number; ok: boolean; err?: string };
  const HIST_KEY = "acm.rotateHistory";

  let keyId = $state(1);
  let algo = $state(0x02);
  let materialHex = $state("");
  let busy = $state(false);
  let history = $state<Hist[]>(loadHistory());

  const selectedAlgo = $derived(algorithmById(algo));
  const requiredHexLen = $derived((selectedAlgo?.key_bytes ?? 0) * 2);
  const materialValid = $derived(
    /^[0-9a-fA-F]*$/.test(materialHex) &&
    materialHex.length === requiredHexLen
  );

  function loadHistory(): Hist[] {
    try { return JSON.parse(localStorage.getItem(HIST_KEY) || "[]"); }
    catch { return []; }
  }
  function saveHistory(h: Hist[]) {
    localStorage.setItem(HIST_KEY, JSON.stringify(h.slice(-20)));
  }

  function randomKey() {
    if (!selectedAlgo) return;
    const buf = new Uint8Array(selectedAlgo.key_bytes);
    crypto.getRandomValues(buf);
    materialHex = Array.from(buf, b => b.toString(16).padStart(2, "0")).join("");
  }

  async function rotate() {
    if (!materialValid) {
      toast("warn", `Нужно ровно ${requiredHexLen} hex-символов`);
      return;
    }
    busy = true;
    let entry: Hist = { ts: Date.now(), key_id: keyId, algo, ok: false };
    try {
      await api.rotateKey(keyId, algo, materialHex);
      entry.ok = true;
      toast("ok", `Ключ #${keyId} (${selectedAlgo?.name}) применён`);
      materialHex = "";
    } catch (e: any) {
      entry.err = e?.error ?? String(e);
      toast("err", `Ошибка: ${entry.err}`);
    } finally {
      busy = false;
      history = [...history, entry];
      saveHistory(history);
    }
  }

  function fmtTime(ts: number) {
    return new Date(ts).toLocaleString("ru-RU");
  }

  function clearHistory() {
    history = [];
    saveHistory(history);
  }
</script>

<div class="space-y-5">
  <Card title="Ротация криптографического ключа"
        subtitle="Передаётся в cryptod, тот проводит self-test и атомарно подменяет провайдер">
    <form
      class="grid grid-cols-1 gap-4 md:grid-cols-[auto_1fr]"
      onsubmit={(e) => { e.preventDefault(); rotate(); }}
    >
      <label class="contents">
        <span class="self-center text-sm text-slate-300">Key ID</span>
        <input
          type="number"
          min="0" max="4294967295"
          bind:value={keyId}
          class="rounded-md border border-slate-700 bg-slate-950 px-3 py-2
                 font-mono text-sm text-slate-100 focus:border-brand-500
                 focus:outline-none focus:ring-1 focus:ring-brand-500"
          required
        />
      </label>

      <label class="contents">
        <span class="self-center text-sm text-slate-300">Алгоритм</span>
        <select
          bind:value={algo}
          class="rounded-md border border-slate-700 bg-slate-950 px-3 py-2
                 font-mono text-sm text-slate-100 focus:border-brand-500
                 focus:outline-none focus:ring-1 focus:ring-brand-500"
        >
          {#each ALGORITHMS as a}
            <option value={a.id} disabled={!a.supported}>
              0x{a.id.toString(16).padStart(2,"0")} —
              {a.name} ({a.key_bytes} B)
              {a.supported ? "" : "[не поддерживается]"}
            </option>
          {/each}
        </select>
      </label>

      <label class="contents">
        <span class="self-start pt-2 text-sm text-slate-300">
          Материал ключа (hex)
        </span>
        <div>
          <textarea
            rows="2"
            bind:value={materialHex}
            placeholder={requiredHexLen + " hex-символов"}
            class="w-full break-all rounded-md border bg-slate-950 px-3 py-2
                   font-mono text-xs leading-relaxed text-slate-100
                   focus:outline-none focus:ring-1
                   {materialHex && !materialValid
                      ? 'border-rose-500 focus:ring-rose-500'
                      : 'border-slate-700 focus:border-brand-500 focus:ring-brand-500'}"
          ></textarea>
          <div class="mt-1 flex items-center justify-between text-xs">
            <span class="font-mono text-slate-500">
              {materialHex.length} / {requiredHexLen}
            </span>
            {#if materialHex && !materialValid}
              <span class="text-rose-400">
                {!/^[0-9a-fA-F]*$/.test(materialHex)
                  ? "только hex-символы"
                  : "длина не совпадает"}
              </span>
            {/if}
          </div>
        </div>
      </label>

      <div></div>
      <div class="flex items-center gap-2">
        <Button variant="secondary" onclick={randomKey}>
          <Shuffle size={14} /> Сгенерировать
        </Button>
        <Button type="submit" disabled={busy || !materialValid}>
          <RotateCw size={14} /> Применить
        </Button>
      </div>
    </form>
  </Card>

  <Card title="Текущий активный ключ"
        subtitle="Запрос через /api/v1/status каждые 2 секунды">
    {#if $status?.active_key_id != null}
      <div class="flex items-center gap-4">
        <span
          class="grid h-12 w-12 place-items-center rounded-md
                 bg-emerald-500/15 text-emerald-400"
        >
          <KeyRound size={22} />
        </span>
        <div class="font-mono">
          <div class="text-2xl text-slate-100">ID #{$status.active_key_id}</div>
          <div class="text-xs text-slate-400">{$status.active_provider}</div>
        </div>
      </div>
    {:else}
      <p class="text-sm text-slate-400">
        Ключ ещё не задан. Сгенерируйте и примените выше.
      </p>
    {/if}
  </Card>

  <Card title="История ротаций (локально, в браузере)"
        subtitle="Будет заменено на audit log из cryptod">
    {#snippet actions()}
      {#if history.length > 0}
        <Button variant="ghost" size="sm" onclick={clearHistory}>
          <Trash2 size={12} /> Очистить
        </Button>
      {/if}
    {/snippet}

    {#if history.length === 0}
      <p class="text-sm text-slate-500">Пока пусто. Сделайте первую ротацию.</p>
    {:else}
      <div class="overflow-hidden rounded-md border border-slate-800">
        <table class="w-full text-sm">
          <thead class="bg-slate-800/50 text-xs uppercase text-slate-400">
            <tr>
              <th class="px-3 py-2 text-left">Время</th>
              <th class="px-3 py-2 text-left">Key ID</th>
              <th class="px-3 py-2 text-left">Алгоритм</th>
              <th class="px-3 py-2 text-left">Результат</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">
            {#each [...history].reverse() as h}
              <tr class="hover:bg-slate-800/30">
                <td class="px-3 py-2 text-slate-400">{fmtTime(h.ts)}</td>
                <td class="px-3 py-2 text-slate-200">#{h.key_id}</td>
                <td class="px-3 py-2 text-slate-300">
                  {algorithmById(h.algo)?.name ?? "?"}
                </td>
                <td class="px-3 py-2">
                  {#if h.ok}
                    <span class="text-emerald-400">✓ ok</span>
                  {:else}
                    <span class="text-rose-400">✗ {h.err}</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </Card>
</div>
