# 2026-05-22 — Operational stack в acm-agent

**Цель:** реализовать Prometheus exporter + REST API + web-UI в `acm-agent`,
получить полную инфраструктуру мониторинга и управления над уже работающим
cryptod. Это первый кусок «operational maturity» — после него заказчик
может реально мониторить и переключать ключи через браузер.

**Итог:** Реализовано и проверено end-to-end на ISM4120I. Все 17 проверок
прошли. SSH-доступ сохранён, состояние модуля чистое после теста.

## Safety-правила сессии

- Никаких изменений в `/etc`, `/usr`, `/opt`, `/var`.
- Cryptod и agent от user, bind на 127.0.0.1.
- UDS-сокет в `/tmp/acm/cryptod.sock`.
- Каждый тест-скрипт имеет `trap EXIT` для cleanup'а.
- После теста SSH-проверка — модуль не «потерян».

## Что добавлено

### 1. `agent/internal/metrics/collector.go` — Prometheus exporter

Pull-based Prometheus Collector. На каждый scrape `/metrics`:

1. Открывает (или переиспользует) UDS к cryptod;
2. Вызывает `GetStatus`;
3. Эмитит метрики в namespace `acm_cryptod_*`:

| Метрика | Тип | Описание |
|---|---|---|
| `acm_cryptod_up` | gauge | 1 если cryptod ответил, 0 иначе |
| `acm_cryptod_uptime_seconds` | gauge | uptime cryptod |
| `acm_cryptod_version_info{version, active_provider}` | gauge=1 | для join'ов в PromQL |
| `acm_cryptod_active_key_id` | gauge | текущий key_id (-1 если нет) |
| `acm_cryptod_packets_sealed_total` | counter | успешные seal'ы |
| `acm_cryptod_packets_opened_total` | counter | успешные open'ы |
| `acm_cryptod_errors_total` | counter | все криптоошибки |
| `acm_cryptod_ipc_scrape_seconds` | gauge | сколько занял scrape (для SLO) |

Плюс стандартные `go_*` и `acm_agent_process_*` через `collectors.NewGoCollector()` и `NewProcessCollector()`.

### 2. `agent/internal/httpapi/api.go` — REST API

| Endpoint | Метод | Описание |
|---|---|---|
| `/api/v1/status` | GET | StatusReport JSON |
| `/api/v1/keys/rotate` | POST | `{key_id, algo, material_hex}` → `{result:ok}` |
| `/healthz` | GET | `200 ok\n` — agent живой |
| `/readyz` | GET | `200 ready\n` если cryptod ответил, 503 иначе |

Cryptod-ошибки маршалятся в HTTP-коды: 501 → 501 Not Implemented, 4xx из cryptod → тот же 4xx.

### 3. `agent/internal/web/{index.html, web.go}` — минимальный UI

Без фреймворка: одна HTML-страница + чистый JS. Polling `/api/v1/status`
каждые 3 секунды, форма ротации ключа с «Random» кнопкой (использует
`crypto.getRandomValues`). Тёмная тема. Embedded в Go-бинарь через
`//go:embed index.html`.

UI отдельным пакетом в `internal/web/` потому что `//go:embed` не умеет
`..` пути.

### 4. `agent/cmd/acm-agent/main.go` — главный бинарь

- Один HTTP-сервер на `:9100` (Prometheus + REST + UI на одном порту).
- Graceful shutdown на SIGTERM/SIGINT.
- Минимальный access-log в `log.Printf` стиле.
- Флаги: `--cryptod-socket` (UDS path), `--listen` (addr).

Размер бинаря: 7.9 МБ (с Prometheus client + embedded UI + go-runtime).

## Грабли по пути (для отчёта)

### №1: Paramiko виснет на backgrounded процессах через SSH

`nohup ... &` через `exec_command` зависал бесконечно. Даже с `disown`.
Причина: bash не отпускает SSH-канал, пока child держит унаследованные
file descriptors stdout/stderr.

**Решение:** использовать `setsid -f` который форкается в новую сессию,
закрывая связь с TTY/каналом. Плюс выполнять всё через **один bash-скрипт
на самом модуле** (`scripts/agent-test-on-module.sh`), который сам
стартует/проверяет/чистит, а paramiko просто вызывает его одним
`exec_command` и читает stdout — один заход.

### №2: На модуле нет ни `curl`, ни `wget`, ни `xxd`

Минимальная Debian-сборка от вендора. Зато:

- **`python3` есть** — используем `urllib.request` для HTTP, `secrets.token_hex(32)` для ключа;
- **`tcpdump`, `iperf3`** есть (видели в recon);
- **OpenSSL CLI** мы поставили в прошлой сессии.

Замена в скрипте: маленькие bash-функции `http_get`, `http_post_json`
поверх `python3 -c "..."`. С обработкой `urllib.error.HTTPError` чтобы
4xx не падал стектрейсом.

### №3: `eval` в bash assert ломается на HTML с кавычками и скобками

`assert "..." "contains '$ui' '...'"` пробует `eval` подставленную
переменную с произвольным контентом. HTML типа `<option>0x10 ... (not yet)</option>`
ломает синтаксис.

**Решение:** для больших ответов класть в файл и `grep -q` напрямую,
без eval.

### №4: Старый процесс держит .deb-файл / бинарь занят

После предыдущей итерации зависший cryptod не позволял SFTP перезаписать
бинарь. Лечится грубым `pkill -9 -f acm-{cryptod,agent}` перед `scp`.

## E2E прогон — финальный

```
=== probes ===
PASS: /healthz returns ok
PASS: /readyz  returns ready

=== status (before rotate) ===
{"version":"0.1.0","running":true,"uptime_s":2,"active_provider":"ring/aes-256-gcm","active_key_id":null,...}
PASS: status has version
PASS: active_key_id is null

=== rotate-key via REST ===
{"result":"ok"}
PASS: rotate returns ok

=== status (after rotate) ===
{..."active_key_id":42,...}
PASS: active_key_id == 42

=== /metrics ===
acm_cryptod_active_key_id 42
acm_cryptod_errors_total 0
acm_cryptod_ipc_scrape_seconds 0.00177592
acm_cryptod_packets_opened_total 0
acm_cryptod_packets_sealed_total 0
acm_cryptod_up 1
acm_cryptod_uptime_seconds 4
acm_cryptod_version_info{active_provider="ring/aes-256-gcm",version="0.1.0"} 1
PASS: acm_cryptod_up == 1
PASS: version_info present
PASS: active_key_id metric == 42
PASS: sealed counter == 0 pre-encdec

=== / (web UI) ===
UI page: 6439 bytes
PASS: / has <title>
PASS: / has 'cryptod status'
PASS: / has rotate-key form

=== encdec-test ===  (poднимает счётчики)
PASS

=== /metrics (after encdec) ===
acm_cryptod_errors_total 1
acm_cryptod_packets_opened_total 9
acm_cryptod_packets_sealed_total 10
PASS: sealed == 10
PASS: opened == 9
PASS: errors == 1

=== agent access log tail ===
GET /healthz 200 0ms 127.0.0.1:55446
GET /readyz 200 1ms 127.0.0.1:55482
GET /api/v1/status 200 0ms 127.0.0.1:55498
POST /api/v1/keys/rotate 200 1ms 127.0.0.1:55510
GET /api/v1/status 200 0ms 127.0.0.1:55522
GET /metrics 200 19ms 127.0.0.1:55538
GET / 200 38ms 127.0.0.1:55544
GET /metrics 200 5ms 127.0.0.1:44252

ALL_TESTS_PASSED
```

Латентность endpoint'ов на модуле:
- `/healthz`, `/readyz`, `/api/v1/status` — **0–1 мс**;
- `/api/v1/keys/rotate` — 1 мс (включая cryptod self-test после rotate);
- `/metrics` (с IPC к cryptod) — **5–19 мс** (первый scrape тяжелее: устанавливает UDS);
- `/` (web UI, 6.4 КБ) — 38 мс на первый запрос.

Это **production-приемлемые числа** на одном Cortex-A53.

## Изменения состояния модуля

После теста:
- `/home/user/acm-uz/{acm-cryptod, acm-agent, acm-cli, acm-encdec-test}` — обновлены.
- `/tmp/acm/cryptod.sock` — удалён в trap EXIT.
- `/tmp/acm/{cryptod.log, agent.log, encdec.out, ui.html, agent-test.sh}` — остались, размером единицы КБ.
- Процессы `acm-cryptod` и `acm-agent` — pkill'ed.
- SSH доступен, post-cleanup-check прошёл.

Никаких изменений в `/etc/*`, `/usr/*`, `/opt/*`, `/var/*`. Сеть не трогали.

## Что доказали в этой сессии

1. ✅ **Prometheus scrape работает** — Zabbix / Grafana / VictoriaMetrics
   могут забирать метрики стандартным pull-pattern'ом.
2. ✅ **REST API позволяет управлять cryptod** из браузера или curl —
   ротация ключа, статус, healthchecks.
3. ✅ **Web UI отдается тем же бинарём** — embedded HTML/JS, не нужен
   отдельный web-сервер.
4. ✅ **Latency приемлема** — 0–1 мс на простые endpoints, 5–19 мс на
   `/metrics` (с IPC), 38 мс на UI.
5. ✅ **Связка работает целиком**: browser → agent → UDS → cryptod →
   ring AES-256-GCM → ARM Crypto Ext → response. Все uровни проверены.

## Что осталось для «полной operational maturity»

- **SNMP агент** в Go (для Zabbix integration в OT-сетях, где Prometheus не используется). См. план — это следующая big item.
- **TLS на agent** (mTLS клиентских сертификатов + сертификат сервера от УЦ заказчика).
- **Syslog forwarder** (события безопасности → SIEM).
- **OAuth/OIDC аутентификация** на web-UI (когда будут realистичные деплои не на 127.0.0.1).
- **Audit log** в SQLite (для соответствия 2814).
- **Svelte SPA** (когда захотим сложные формы — multi-step rotation,
  policy editor и т.п.).

## Финальные метрики дня 2

- **Unit-тестов:** 34 (rust) + 0 (Go) = 34
- **Integration-тестов на железе:** 3 (auth, encdec, agent-ops)
- **Коммитов:** 8 (за весь день)
- **Бинарей под aarch64:** 4 (cryptod, agent, cli, encdec-test) + 1 amd64 (controller)
- **HTTP endpoints на агенте:** 5 публичных
- **Prometheus метрик публикуется:** 8 (наших) + ~30 (стандартных go/process)
- **Состояние модуля:** чистое
