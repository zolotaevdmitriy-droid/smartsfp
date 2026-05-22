# 2026-05-22 — Operational UI: Svelte SPA + sysmon + мониторинг процессов

**Цель сессии:** сделать «полноценную web-админку как у роутера» — современный
SPA на Svelte вместо одностраничного HTML, плюс мониторинг компонентов
системы (CPU/RAM/диск/температура/сеть) и наших собственных процессов
(acm-cryptod, acm-agent).

**Итог:** Реализовано end-to-end, проверено вживую на ISM4120I (192.168.0.99).
Все секции отдают живые данные с реального железа. SSH-доступ к модулю
сохранён, ничего в `/etc /usr /var` не тронуто.

## Safety-правила сессии

Те же что в предыдущих:
- никаких изменений в `/etc`, `/usr`, `/opt`, `/var` модуля;
- agent и cryptod от user (не root), bind на `127.0.0.1:9100`;
- доступ из браузера через SSH-туннель `scripts/open_agent_ui.py`;
- все тест-скрипты с явным cleanup в `finally`/`trap EXIT`.

## Что добавлено за сессию

### 1. Svelte 5 + TypeScript + Tailwind 4 SPA (`web-ui/`)

**Стек:**
- `svelte@5.16` (с новыми runes: `$state`, `$derived`, `$effect`, `$props`, `$snippet`);
- `vite@6`, `@sveltejs/vite-plugin-svelte`;
- `tailwindcss@4` + `@tailwindcss/vite` (с inline-конфигом через `@theme` в `app.css`, **без** `tailwind.config.js`);
- `lucide-svelte@0.469` для иконок (важно: НЕ `@lucide/svelte` — это React, выдало `ETARGET`);
- TypeScript strict.

**Структура `web-ui/src/`:**
```
lib/
  api.ts      типы StatusReport/SystemSnapshot + fetch-обёртка
  stores.ts   route/status/sysmon/toast stores + installRouter() + helpers
components/
  Sidebar.svelte    — nav с lucide-иконками, hash-router
  TopBar.svelte     — title + UP/DOWN бейдж
  Card.svelte       — секция с title/subtitle/actions snippet
  Stat.svelte       — KPI-плитка с иконкой и tone
  Button.svelte     — primary/secondary/ghost/danger
  Toasts.svelte     — стек уведомлений в углу
  ProgressBar.svelte — авто-tone (зелёный <70%, амбер 70-90%, красный >90%)
pages/
  Dashboard.svelte  — KPI + crypto throughput + system health + processes + hardware
  Keys.svelte       — форма ротации с hex-валидацией + history в localStorage
  Logs.svelte       — пока заглушка (нужен SSE-endpoint)
  Settings.svelte   — пока заглушка (нужны mgmt REST endpoints)
App.svelte          — sidebar + topbar + router + toasts
```

**Bundle metrics** (после `vite build`):
- index.html: 0.58 KB
- index-*.css: 26.58 KB → 5.45 KB gzip
- index-*.js: 103.19 KB → 33.41 KB gzip
- **Итого SPA: ~130 KB on disk, ~40 KB gzip on wire**

Это очень мало для такого количества функционала — Svelte 5 + Tailwind 4 + TS дают компактные бандлы.

### 2. Build pipeline: `./dev.sh build` теперь включает SPA

Дополнен `dev.sh build`:
```
1. cd web-ui && [ -d node_modules ] || npm install
2. npm run build           → web-ui/dist/
3. rm -rf agent/internal/web/dist && cp -r web-ui/dist agent/internal/web/dist
4. cargo build (Rust)
5. go build (Go) — embed подхватывает agent/internal/web/dist
```

Кеш node_modules через named docker volume `acm-uz-npm-cache` (mount к `/root/.npm`) — повторные сборки не качают зависимости.

Файл `agent/internal/web/web.go` теперь делает `//go:embed all:dist` (вместо одного `index.html`). Сабпакет так как `go:embed` не умеет `..` пути.

`/agent/internal/web/dist/` и `/web-ui/dist/` — оба в `.gitignore`.

### 3. `sysmon` package: чтение реальных метрик с модуля

Новый пакет `agent/internal/sysmon` читает:

| Источник | Что снимает |
|---|---|
| `/proc/uptime` | uptime модуля |
| `/proc/loadavg` | load avg 1m / 5m / 15m |
| `/proc/stat` | CPU (overall + per-core, %, delta между вызовами) |
| `/proc/meminfo` | MemTotal, MemAvailable, MemFree, Buffers, Cached |
| `syscall.Statfs` | размер/used/% для `/`, `/var`, `/home` |
| `/sys/class/hwmon/hwmon*` | имя датчика + `temp1_input` (mC → °C) |
| `/proc/net/dev` + `/sys/class/net/*/operstate,mtu` | RX/TX bytes/packets/errors/dropped + UP/DOWN + delta-rate (bps) |
| `/proc/[pid]/comm,stat,status,exe` | наши процессы: PID, RSS, CPU%, threads, start_time, uptime, бинарь на диске (size, mtime) |

`Reader{}` хранит prev-state для CPU/net/process delta-вычислений. Threadsafe (`sync.Mutex`).

**Канонические grаble Linux при парсинге `/proc/[pid]/stat`:**
- `comm` в скобках МОЖЕТ содержать пробелы → используем `strings.LastIndexByte(line, ')')` чтобы корректно сплитить;
- CPU% через jiffies (utime+stime), деление на `sysconf(_SC_CLK_TCK)` (=100 на Linux) и wall-clock delta;
- `starttime` (поле 22) в jiffies-since-boot, прибавляем `btime` из `/proc/stat` для unix seconds.

### 4. `acm_module_*` и `acm_process_*` Prometheus метрики

Расширил `agent/internal/metrics/`:
- `collector.go` — старые `acm_cryptod_*` (из UDS)
- `sysmon.go` (новый) — `acm_module_*` (CPU/RAM/disk/temp/net) + `acm_process_*` (наши бинари)

Полный список новых метрик:

```
acm_module_uptime_seconds
acm_module_load_average{period="1m|5m|15m"}
acm_module_cpu_busy_pct
acm_module_cpu_core_busy_pct{core}
acm_module_memory_total_bytes
acm_module_memory_available_bytes
acm_module_memory_used_pct
acm_module_filesystem_size_bytes{mount}
acm_module_filesystem_used_bytes{mount}
acm_module_filesystem_used_pct{mount}
acm_module_temperature_celsius{sensor}
acm_module_interface_up{iface}
acm_module_interface_rx_bytes_total{iface}
acm_module_interface_tx_bytes_total{iface}
acm_module_interface_rx_packets_total{iface}
acm_module_interface_tx_packets_total{iface}
acm_module_interface_rx_errors_total{iface}
acm_module_interface_tx_errors_total{iface}
acm_module_interface_rx_bps{iface}
acm_module_interface_tx_bps{iface}

acm_process_up{name}
acm_process_memory_rss_bytes{name}
acm_process_cpu_percent{name}
acm_process_threads{name}
acm_process_start_time_seconds{name}
acm_process_uptime_seconds{name}
acm_process_binary_size_bytes{name,path}
acm_process_binary_mtime_seconds{name,path}
```

### 5. `/api/v1/system` REST endpoint

Принимает GET, возвращает полный JSON-снимок:
```json
{
  "taken_at": "2026-05-22T...",
  "uptime_s": ...,
  "load_avg": [..., ..., ...],
  "cpu": { "cores": 2, "busy_pct": 0.0, "per_core_pct": [0.0, 0.0] },
  "memory": { "total_kb", "available_kb", "free_kb", "buffers_kb", "cached_kb", "used_pct" },
  "filesystems": [{"mount":"/","size_kb":...,"used_kb":...,"used_pct":...}],
  "sensors": [{"name":"tmp1075","temp_c":57.0}],
  "interfaces": [{"name":"gbe0","up":true,"rx_bytes":...,"tx_bps_now":...,...}],
  "processes": [{"name":"acm-cryptod","pid":12110,"rss_kb":1012,"binary_size":1513824,...}]
}
```

UI polling его каждые 2 секунды параллельно с `/api/v1/status`.

### 6. Dashboard теперь показывает живой sysmon

7 секций сверху вниз:
1. **4 KPI-плитки**: Состояние, Uptime, Активный ключ, Ошибки крипто.
2. **Crypto throughput** — packets sealed/opened с скоростью.
3. **CPU** карточка — общая загрузка + per-core ProgressBar.
4. **Память** — used% + Free/Buffers/Cached.
5. **Диск** — три ProgressBar для `/`, `/var`, `/home`.
6. **Температура** — все hwmon-датчики (видим `nt25l90` и `tmp1075`).
7. **Сетевые интерфейсы** — таблица с UP/DOWN, RX/TX сейчас (Mbps), всего, ошибки.
8. **Процессы ACM-UZ** — таблица с PID, RSS, CPU%, threads, uptime, binary size+mtime для каждого нашего бинаря.
9. **Аппарат** — статика (CPU model, HW crypto, OS, mgmt IP).

В таблице процессов: **stale-binary detection** — если `binary_mtime > start_time + 60s`, показывает amber warning «бинарь на диске обновлён, перезапустите процесс».

## Грабли по пути (для отчёта)

### №1: `@lucide/svelte` ≠ `lucide-svelte`

Первая попытка `npm install` упала с `ETARGET No matching version found for @lucide/svelte@^0.469.0`. Оказалось:
- `@lucide/svelte` — пакет для **React** (npm scope `@lucide/`)
- `lucide-svelte` — то что нужно для Svelte

Заменил импорты через `sed`, `npm install` прошёл.

### №2: `//go:embed` не умеет `..`

Сначала пробовал в `agent/cmd/acm-agent/main.go`:
```go
//go:embed all:../../internal/web
```
Не работает: `pattern ../../internal/web: cannot embed files in parent directory`.

Решение: re-export FS из подпакета рядом с ассетами:
```go
// agent/internal/web/web.go
//go:embed all:dist
var raw embed.FS
func FS() fs.FS { sub, _ := fs.Sub(raw, "dist"); return sub }
```

### №3: Иконка «Ключи» (`Key`) выглядела как ♂

Lucide `Key` рисует ключ в стиле «зеркальный знак мужского пола». Заменил на `KeyRound` — однозначно ключ.

### №4: Robust навигация

На всякий случай переписал router-логику:
- регистрация `hashchange` listener'а из `App.svelte onMount` (а не module-init side effect);
- `navigate(name)` теперь ставит `location.hash` И **дополнительно** `route.set(name)` — belt-and-braces.

### №5: Module Debian минимальный

Уже знали из прошлой сессии: на модуле нет `curl`, `wget`, `xxd`. Все скрипты используют `python3` + `urllib.request`. Здесь не было сюрпризов.

### №6: paramiko + agent foreground

`acm-agent` в фоне через `nohup ... &` снова повесил SSH-канал — как с cryptod в прошлой сессии. Решение то же: `setsid -f` для надёжного fork.

## Прогон проверки на ISM4120I (живые данные)

```
$ python -c "...urllib... /api/v1/system"
uptime 161359.28
cpu busy 0 per_core [0, 0]
mem used% 60.2
sensors:
   nt25l90 65.9 C
   tmp1075 57 C
interfaces:
   br0  up rx 195336992  tx 798461044  rate 0 / 0 bps
   gbe0 up rx 583450     tx 979248880  rate 0 / 0 bps
   gbe1 up rx 1180567936 tx 799045350  rate 0 / 0 bps
processes:
   acm-cryptod  pid 12110  rss 1012 KB  threads 3  binary 1513824 bytes
   acm-agent    pid 12353  rss 6948 KB  threads 7  binary 7995544 bytes
```

**Наблюдения:**
- Модуль практически простаивает (CPU 0%, load avg тоже 0).
- Память 60% — это с учётом DPDK hugepages 400 МБ зарезервированных при boot.
- `tmp1075 = 57.0 °C` — нормальная температура для SFP в работе.
- `gbe0` имеет накопленные сотни МБ TX — это вероятно весь сетевой трафик через bridge (модуль bridges gbe0 ↔ gbe1).
- **Наши бинари очень скромные**: cryptod 1 МБ RSS, agent 7 МБ RSS — суммарно <8 МБ под всё крипто+UI+API.

## Изменения состояния модуля

После сессии на модуле живут:
- `/home/user/acm-uz/acm-cryptod` (1.51 МБ, новый бинарь Rust)
- `/home/user/acm-uz/acm-agent` (8.0 МБ, новый бинарь Go с Svelte embed)
- `/home/user/acm-uz/acm-cli` (2.42 МБ)
- `/home/user/acm-uz/acm-encdec-test` (2.49 МБ)
- `/tmp/acm/cryptod.sock` (UDS, активен пока процесс жив)
- `/tmp/acm/cryptod.log`, `/tmp/acm/agent.log` — текущие логи
- 2 процесса: `acm-cryptod` (PID 12110), `acm-agent` (PID 12353)

Изменений в `/etc /usr /opt /var`: **0**. SSH-сессии — живые.

## Что доказали в этой сессии

1. ✅ **Готовый build pipeline для современного SPA** в embedded Linux-устройстве. `./dev.sh build` собирает Svelte + Rust + Go одной командой.
2. ✅ **Polished admin UI**: sidebar, sticky topbar, status badge с пульсацией, dark theme, lucide иконки, tailwind utilities, ProgressBar с auto-tone, toasts. Выглядит как современная админка роутера/ASIC'а.
3. ✅ **Live мониторинг компонентов**: CPU/RAM/диск/температура/сеть — все данные с реального железа, не моки.
4. ✅ **Мониторинг наших процессов**: PID, RSS, CPU%, threads, binary size + mtime — фундамент для production-аудита.
5. ✅ **Stale-binary detection**: UI автоматически замечает, если выложил новый бинарь но забыл перезапустить процесс.
6. ✅ **Prometheus metrics дополнены `acm_module_*` и `acm_process_*`** — Zabbix/Grafana могут строить полноценные дашборды без дополнительного `node_exporter`.

## Что осталось для «полной operational maturity»

- **SSE endpoint /api/v1/events/stream** + Logs page с live-tail (вынесли в следующую итерацию).
- **REST для Settings page**: hostname, network config (read-only), NTP, SSH keys.
- **TLS на agent** (mTLS клиентские сертификаты + сертификат сервера от УЦ заказчика). Сейчас HTTP.
- **OIDC/Keycloak login** для web-UI.
- **SNMP агент** в Go (для интеграции в Zabbix через классический pull).
- **Audit log в SQLite** для соответствия требованиям 2814.
- **i18n** (ru / uz-latn / uz-cyrl).
- **Замена localStorage history на server-side audit log** в Keys.svelte.

## Метрики дня (итог)

| Метрика | Значение |
|---|---|
| Новых файлов в репо | ~25 (Svelte components + pages + sysmon + collectors) |
| LOC добавлено | ~2000 (Svelte + TS + Go) |
| Unit-тестов | 34 (rust workspace, без изменений) |
| Интеграционных тестов на железе | 4 (auth, recon, encdec, agent-ops + sysmon) |
| Размер acm-agent | 7.9 МБ → 8.0 МБ (после Svelte SPA + sysmon) |
| Размер SPA bundle | 103 KB JS + 27 KB CSS = 130 KB on disk, 40 KB gzip |
| Prometheus metrics published | 8 (cryptod) + 22 (module) + 8 (process) + ~30 (go runtime) = ~68 |
| HTTP endpoints | /metrics, /api/v1/{status,system,keys/rotate}, /healthz, /readyz, / (UI) |
| Изменений в `/etc /usr /opt /var` | 0 |
