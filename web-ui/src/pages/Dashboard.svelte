<script lang="ts">
  import { status, statusError, sysmon, fmtDuration, fmtN, fmtBytes, fmtBps } from "../lib/stores";
  import Card from "../components/Card.svelte";
  import Stat from "../components/Stat.svelte";
  import ProgressBar from "../components/ProgressBar.svelte";
  import {
    KeyRound, Lock, Unlock, ShieldAlert, Clock,
    Cpu, MemoryStick, HardDrive, Thermometer,
    ArrowDownToLine, ArrowUpFromLine, ServerCog, Activity,
    Boxes, CircleDot, CircleOff,
  } from "lucide-svelte";

  // Snapshot of previous-tick crypto counters for live rates.
  let prev: { sealed: number; opened: number; t: number } | null = $state(null);
  let sealedRate = $state(0);
  let openedRate = $state(0);

  $effect(() => {
    if (!$status) return;
    const now = performance.now();
    if (prev) {
      const dt = (now - prev.t) / 1000;
      if (dt > 0.1) {
        sealedRate = Math.max(0, ($status.packets_sealed - prev.sealed) / dt);
        openedRate = Math.max(0, ($status.packets_opened - prev.opened) / dt);
      }
    }
    prev = { sealed: $status.packets_sealed, opened: $status.packets_opened, t: now };
  });

  function ratePerSec(n: number): string {
    if (n < 0.05) return "—";
    if (n < 100) return n.toFixed(1) + "/с";
    return Math.round(n).toString() + "/с";
  }

  function tempTone(c: number): "ok" | "warn" | "err" {
    if (c < 70) return "ok";
    if (c < 85) return "warn";
    return "err";
  }

  function fmtMtime(unixS: number): string {
    if (unixS === 0) return "—";
    return new Date(unixS * 1000).toLocaleString("ru-RU");
  }
  function fmtBinarySize(n: number): string {
    if (n === 0) return "—";
    if (n < 1024) return n + " B";
    if (n < 1024 * 1024) return (n / 1024).toFixed(0) + " KB";
    return (n / 1024 / 1024).toFixed(2) + " MB";
  }
  function fmtStartTime(unixS: number): string {
    if (unixS === 0) return "—";
    return new Date(unixS * 1000).toLocaleString("ru-RU");
  }
</script>

<div class="space-y-5">
  <!-- ====== KPI tiles ====== -->
  <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
    <Stat
      label="Состояние"
      value={$statusError ? "DOWN" : $status?.running ? "UP" : "..."}
      sub={$status?.active_provider ?? "—"}
      icon={ServerCog}
      tone={$statusError ? "err" : $status?.running ? "ok" : "neutral"}
    />
    <Stat
      label="Uptime"
      value={$sysmon ? fmtDuration(Math.floor($sysmon.uptime_s)) : "—"}
      sub={$status ? "cryptod " + fmtDuration($status.uptime_s) : ""}
      icon={Clock}
    />
    <Stat
      label="Активный ключ"
      value={$status?.active_key_id ?? "—"}
      sub={$status?.active_key_id != null ? "ID#" : "не задан"}
      icon={KeyRound}
      tone={$status?.active_key_id != null ? "ok" : "warn"}
    />
    <Stat
      label="Ошибки крипто"
      value={$status ? fmtN($status.crypto_errors) : "—"}
      sub="суммарно"
      icon={ShieldAlert}
      tone={($status?.crypto_errors ?? 0) > 0 ? "warn" : "neutral"}
    />
  </div>

  <!-- ====== Crypto throughput ====== -->
  <Card title="Криптографические операции"
        subtitle="Счётчики через UDS к cryptod, обновление 2 с">
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div class="rounded-md border border-slate-800 bg-slate-950/50 p-4">
        <div class="flex items-center justify-between">
          <span class="flex items-center gap-2 text-sm text-slate-300">
            <Lock size={14} class="text-emerald-400" />
            Зашифровано (sealed)
          </span>
          <span class="font-mono text-xs text-slate-500">
            {ratePerSec(sealedRate)}
          </span>
        </div>
        <div class="mt-2 font-mono text-3xl text-emerald-300">
          {$status ? fmtN($status.packets_sealed) : "—"}
        </div>
      </div>
      <div class="rounded-md border border-slate-800 bg-slate-950/50 p-4">
        <div class="flex items-center justify-between">
          <span class="flex items-center gap-2 text-sm text-slate-300">
            <Unlock size={14} class="text-sky-400" />
            Расшифровано (opened)
          </span>
          <span class="font-mono text-xs text-slate-500">
            {ratePerSec(openedRate)}
          </span>
        </div>
        <div class="mt-2 font-mono text-3xl text-sky-300">
          {$status ? fmtN($status.packets_opened) : "—"}
        </div>
      </div>
    </div>
  </Card>

  <!-- ====== System Health (live from /api/v1/system) ====== -->
  <div class="grid grid-cols-1 gap-5 lg:grid-cols-2">
    <Card title="CPU"
          subtitle={$sysmon
            ? `${$sysmon.cpu.cores} ядра · load avg ${$sysmon.load_avg.map(x=>x.toFixed(2)).join(" / ")}`
            : "—"}>
      {#snippet actions()}
        <Cpu size={16} class="text-slate-500" />
      {/snippet}
      {#if $sysmon}
        <div class="space-y-3">
          <ProgressBar
            pct={$sysmon.cpu.busy_pct}
            label="Общая загрузка"
            value={$sysmon.cpu.busy_pct.toFixed(1) + "%"}
          />
          {#each $sysmon.cpu.per_core_pct as pct, i}
            <ProgressBar
              pct={pct}
              label={"Ядро " + i}
              value={pct.toFixed(1) + "%"}
            />
          {/each}
        </div>
      {:else}
        <p class="text-sm text-slate-500">загрузка...</p>
      {/if}
    </Card>

    <Card title="Память"
          subtitle={$sysmon
            ? `${fmtBytes($sysmon.memory.total_kb * 1024)} всего, ${fmtBytes($sysmon.memory.available_kb * 1024)} доступно`
            : "—"}>
      {#snippet actions()}
        <MemoryStick size={16} class="text-slate-500" />
      {/snippet}
      {#if $sysmon}
        <ProgressBar
          pct={$sysmon.memory.used_pct}
          label="Использовано"
          value={fmtBytes(($sysmon.memory.total_kb - $sysmon.memory.available_kb) * 1024)
            + " · " + $sysmon.memory.used_pct.toFixed(1) + "%"}
        />
        <dl class="mt-3 grid grid-cols-2 gap-y-1.5 text-xs">
          <dt class="text-slate-500">Free</dt>
          <dd class="text-right font-mono text-slate-200">{fmtBytes($sysmon.memory.free_kb * 1024)}</dd>
          <dt class="text-slate-500">Buffers</dt>
          <dd class="text-right font-mono text-slate-200">{fmtBytes($sysmon.memory.buffers_kb * 1024)}</dd>
          <dt class="text-slate-500">Cached</dt>
          <dd class="text-right font-mono text-slate-200">{fmtBytes($sysmon.memory.cached_kb * 1024)}</dd>
        </dl>
      {:else}
        <p class="text-sm text-slate-500">загрузка...</p>
      {/if}
    </Card>

    <Card title="Диск"
          subtitle="Разделы eMMC модуля">
      {#snippet actions()}
        <HardDrive size={16} class="text-slate-500" />
      {/snippet}
      {#if $sysmon && $sysmon.filesystems.length > 0}
        <div class="space-y-3">
          {#each $sysmon.filesystems as fs (fs.mount)}
            <ProgressBar
              pct={fs.used_pct}
              label={fs.mount}
              value={fmtBytes(fs.used_kb * 1024) + " / " + fmtBytes(fs.size_kb * 1024)}
            />
          {/each}
        </div>
      {:else}
        <p class="text-sm text-slate-500">загрузка...</p>
      {/if}
    </Card>

    <Card title="Температура"
          subtitle="Датчики hwmon на плате модуля">
      {#snippet actions()}
        <Thermometer size={16} class="text-slate-500" />
      {/snippet}
      {#if $sysmon && $sysmon.sensors.length > 0}
        <div class="space-y-2.5">
          {#each $sysmon.sensors as s (s.name)}
            {@const tone = tempTone(s.temp_c)}
            <div class="flex items-center justify-between">
              <span class="font-mono text-sm text-slate-300">{s.name}</span>
              <span class="font-mono text-lg
                {tone === 'ok'  ? 'text-emerald-300' : ''}
                {tone === 'warn'? 'text-amber-300'   : ''}
                {tone === 'err' ? 'text-rose-300'    : ''}">
                {s.temp_c.toFixed(1)} °C
              </span>
            </div>
          {/each}
        </div>
      {:else}
        <p class="text-sm text-slate-500">нет данных</p>
      {/if}
    </Card>
  </div>

  <!-- ====== Network interfaces ====== -->
  <Card title="Сетевые интерфейсы"
        subtitle="Live RX/TX из /proc/net/dev">
    {#if $sysmon && $sysmon.interfaces.length > 0}
      <div class="overflow-hidden rounded-md border border-slate-800">
        <table class="w-full text-sm">
          <thead class="bg-slate-800/50 text-xs uppercase tracking-wide text-slate-400">
            <tr>
              <th class="px-3 py-2 text-left">Интерфейс</th>
              <th class="px-3 py-2 text-left">Статус</th>
              <th class="px-3 py-2 text-right">
                <div class="inline-flex items-center gap-1.5">
                  <ArrowDownToLine size={12} /> Сейчас
                </div>
              </th>
              <th class="px-3 py-2 text-right">
                <div class="inline-flex items-center gap-1.5">
                  <ArrowUpFromLine size={12} /> Сейчас
                </div>
              </th>
              <th class="px-3 py-2 text-right">RX (всего)</th>
              <th class="px-3 py-2 text-right">TX (всего)</th>
              <th class="px-3 py-2 text-right">Ошибки</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">
            {#each $sysmon.interfaces as ni (ni.name)}
              <tr>
                <td class="px-3 py-2 font-medium text-slate-200">{ni.name}</td>
                <td class="px-3 py-2">
                  {#if ni.up}
                    <span class="rounded-full border border-emerald-500/30 bg-emerald-500/10
                                 px-1.5 py-0.5 text-[10px] text-emerald-300">UP</span>
                  {:else}
                    <span class="rounded-full border border-slate-700 bg-slate-800
                                 px-1.5 py-0.5 text-[10px] text-slate-400">DOWN</span>
                  {/if}
                </td>
                <td class="px-3 py-2 text-right text-emerald-300">
                  {fmtBps(ni.rx_bps_now)}
                </td>
                <td class="px-3 py-2 text-right text-sky-300">
                  {fmtBps(ni.tx_bps_now)}
                </td>
                <td class="px-3 py-2 text-right text-slate-400">
                  {fmtBytes(ni.rx_bytes)}
                </td>
                <td class="px-3 py-2 text-right text-slate-400">
                  {fmtBytes(ni.tx_bytes)}
                </td>
                <td class="px-3 py-2 text-right">
                  {#if ni.rx_errors + ni.tx_errors > 0}
                    <span class="text-rose-400">{ni.rx_errors + ni.tx_errors}</span>
                  {:else}
                    <span class="text-slate-600">0</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="text-sm text-slate-500">загрузка...</p>
    {/if}
  </Card>

  <!-- ====== Our processes (acm-cryptod, acm-agent) ====== -->
  <Card title="Процессы ACM-UZ"
        subtitle="Текущее состояние наших бинарей и файлы на диске">
    {#snippet actions()}
      <Boxes size={16} class="text-slate-500" />
    {/snippet}
    {#if $sysmon && $sysmon.processes.length > 0}
      <div class="overflow-hidden rounded-md border border-slate-800">
        <table class="w-full text-sm">
          <thead class="bg-slate-800/50 text-xs uppercase tracking-wide text-slate-400">
            <tr>
              <th class="px-3 py-2 text-left">Бинарь</th>
              <th class="px-3 py-2 text-left">Состояние</th>
              <th class="px-3 py-2 text-right">PID</th>
              <th class="px-3 py-2 text-right">RSS</th>
              <th class="px-3 py-2 text-right">CPU</th>
              <th class="px-3 py-2 text-right">Threads</th>
              <th class="px-3 py-2 text-right">Uptime</th>
              <th class="px-3 py-2 text-right">Файл (mtime)</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800 font-mono">
            {#each $sysmon.processes as p (p.name)}
              <tr class="hover:bg-slate-800/30">
                <td class="px-3 py-2">
                  <div class="font-medium text-slate-200">{p.name}</div>
                  {#if p.exe_path}
                    <div class="text-[10px] text-slate-500">{p.exe_path}</div>
                  {/if}
                </td>
                <td class="px-3 py-2">
                  {#if p.running}
                    <span class="inline-flex items-center gap-1 rounded-full border border-emerald-500/30
                                 bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-300">
                      <CircleDot size={10} /> {p.state || "running"}
                    </span>
                  {:else}
                    <span class="inline-flex items-center gap-1 rounded-full border border-rose-500/30
                                 bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-300">
                      <CircleOff size={10} /> stopped
                    </span>
                  {/if}
                </td>
                <td class="px-3 py-2 text-right text-slate-300">
                  {p.running ? p.pid : "—"}
                </td>
                <td class="px-3 py-2 text-right">
                  {#if p.running}
                    <span class="text-slate-200">{fmtBytes(p.rss_kb * 1024)}</span>
                  {:else}—{/if}
                </td>
                <td class="px-3 py-2 text-right">
                  {#if p.running}
                    <span class="text-slate-200">{p.cpu_pct.toFixed(1)}%</span>
                  {:else}—{/if}
                </td>
                <td class="px-3 py-2 text-right text-slate-300">
                  {p.running ? p.threads : "—"}
                </td>
                <td class="px-3 py-2 text-right text-slate-300">
                  {p.running ? fmtDuration(Math.floor(p.uptime_s)) : "—"}
                </td>
                <td class="px-3 py-2 text-right">
                  <div class="text-slate-200">{fmtBinarySize(p.binary_size)}</div>
                  <div class="text-[10px] text-slate-500">
                    {fmtMtime(p.binary_mtime_unix_s)}
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <!-- Stale binary detection -->
      {#each $sysmon.processes as p}
        {#if p.running && p.binary_size > 0}
          {@const ageSec = Date.now() / 1000 - p.binary_mtime_unix_s}
          {#if ageSec < (Date.now() / 1000 - p.start_unix_s) - 60}
            <div class="mt-2 flex items-center gap-2 rounded-md border border-amber-500/30
                        bg-amber-500/5 px-3 py-1.5 text-xs text-amber-200">
              <ShieldAlert size={12} />
              <span>
                <span class="font-mono">{p.name}</span> на диске обновлён позже,
                чем запущен процесс — выгрузка новее, чем работающий бинарь.
                Перезапустите.
              </span>
            </div>
          {/if}
        {/if}
      {/each}
    {:else}
      <p class="text-sm text-slate-500">загрузка...</p>
    {/if}
  </Card>

  <!-- ====== Static hardware info ====== -->
  <Card title="Аппарат"
        subtitle="Smart SFP ISM4120I · Marvell Armada 3720">
    <dl class="grid grid-cols-1 gap-y-1.5 text-sm sm:grid-cols-2 sm:gap-x-8">
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">CPU</dt>
        <dd class="font-mono text-slate-200">2× Cortex-A53 @ 1.2 ГГц</dd>
      </div>
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">HW crypto</dt>
        <dd class="font-mono text-slate-200">EIP-97 · ARMv8 Crypto Ext</dd>
      </div>
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">RAM</dt>
        <dd class="font-mono text-slate-200">
          {$sysmon ? fmtBytes($sysmon.memory.total_kb * 1024) : "1.0 ГБ"}
        </dd>
      </div>
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">Storage</dt>
        <dd class="font-mono text-slate-200">4 ГБ eMMC</dd>
      </div>
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">OS</dt>
        <dd class="font-mono text-slate-200">Debian 12 / kernel 6.1</dd>
      </div>
      <div class="flex items-center justify-between">
        <dt class="text-slate-400">Mgmt IP</dt>
        <dd class="font-mono text-slate-200">192.168.0.99/24</dd>
      </div>
    </dl>
  </Card>
</div>
