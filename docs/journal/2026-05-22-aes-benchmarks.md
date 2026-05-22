# 2026-05-22 — Бенчмарки AES на ISM4120I

**Участники:** Д. Золотаев + Claude
**Цель:** измерить реальную производительность AES-GCM/AES-CBC на ISM4120I —
через ARM Crypto Extensions (CPU) и через Marvell SAM (EIP-97 HW-ускоритель,
доступ через MUSDK). Получить числа, на которые можно опираться при
проектировании датапаса и при разговоре с заказчиком.
**Итог (заполняется в конце):** TBD

## Контекст к началу сессии

* Репозиторий `smartsfp` скелетирован, четыре бинаря собраны вчера
  (commit 1bdbe26), запущены на 192.168.0.99 — печатают hello.
* По вчерашней разведке (`scripts/recon_sfp.py`, `recon_output.txt` локально):
  на модуле стоят **DPDK 25.0**, **MUSDK** с примерами `musdk_sam_*`,
  **hugepages 200×2MB** зарезервированы при boot.
* `musdk_sam_kat` — KAT-инструмент для SAM, запускается через CMA/DMA,
  требует root (под `user` падает с `CMA: open() failed`, см.
  `scripts/probe_crypto.py` сегодня утром).
* `openssl` CLI на модуле **не установлен**, но `libssl 3.0.15` есть
  (используется SSH).
* Доступ к модулю — SSH через jumphost root@178.104.223.171 (пароль),
  на самом модуле учётка `user`/`PleaseChangeTheUserPassword`, **sudo
  доступен с тем же паролем**.
* Перед бенчмарками будем `apt install openssl` — это единственное
  изменение состояния модуля на этой сессии.

## Шаг 1 — Разведка крипто-инструментов под user

См. полный вывод в скрипте `scripts/probe_crypto.py` от 2026-05-22.

Ключевое:

| Артефакт | Под user | Заметка |
|---|---|---|
| `openssl` CLI | ❌ нет | `command -v openssl` пусто |
| `musdk_sam_kat` | ⚠️ help работает | реальный запуск ломается на CMA |
| `musdk_sam_single` | ❌ `CMA: open() failed` | DMA требует root |
| `/proc/crypto` | (пусто) | kernel CRYPTO API не собран — SAM используется в user-space |
| `/dev/uio*` | не виден под user | скрыты permissions |
| CPU features | `aes pmull sha1 sha2 crc32` | ARMv8 Crypto Extensions есть ✓ |
| `iperf3`, `tcpdump` | установлены | для сетевых тестов |
| Python `cryptography` | не установлен | |
| `sudo -n` | требует пароль | штатно |

**Вывод:** под user мы можем только запускать наши собственные бинари с
CPU-крипто (через ARM Crypto Ext). Для SAM нужен root через sudo.

## Шаг 2 — Попытка прогнать `openssl speed` и `musdk_sam_kat` через `sudo`

Запустил `scripts/bench_aes.py`. Команда `sudo -S -p '' whoami` с паролем
`PleaseChangeTheUserPassword`:

    sudo: unable to resolve host smart-sfp: Temporary failure in name resolution
    user is not in the sudoers file.
    exit=1

**Наблюдение:** учётка `user` **не входит в sudoers** на этом модуле.
Пароль для sudo тут вообще ни при чём — sudoers политика не позволяет.
Все последующие команды (`apt install openssl`, `musdk_sam_kat`) — упали по
той же причине. Полный лог попыток: `scripts/output/bench-20260522-090534.txt`.

Сообщение про `unable to resolve host smart-sfp` — побочное, лечится
добавлением `127.0.1.1 smart-sfp` в `/etc/hosts` (там уже есть, но видимо
не подхватился), к нашему вопросу не относится.

**Решение:** для прогона бенчмарков нужен **прямой root-доступ** либо
ручное добавление `user` в `sudoers`. По спецификации модуля заводской
пароль root — `PleaseChangeTheRootPassword` (отличается от user). Уточняем
у владельца модуля и продолжаем тогда.

**Не делаем сами** — никаких изменений `/etc/sudoers` без подтверждения.

## Шаг 3 — Параллельная работа: AES-GCM провайдер на хосте

Пока root-доступ согласовывается, продолжаем без модуля.

### 3.1. `KeyHandle` + `CryptoProvider` trait в `acm-crypto`

Подключены крейты:

* `aes-gcm = "0.10"` (с фичей `"aes"` для подхвата ARMv8 Crypto Ext);
* `zeroize` — `KeyHandle` теперь `ZeroizeOnDrop`, материал ключа
  стирается при drop'е;
* `subtle` — для будущих constant-time сравнений;
* `hex` в dev-dependencies — для KAT-векторов в тестах.

Расширен trait `CryptoProvider`: добавлены типизированные ошибки
`InvalidKey { expected, got }`, `InvalidNonce`, `BufferTooSmall`,
`AlgorithmMismatch`. `KeyHandle::new()` валидирует длину ключа против
`AlgoId::key_len()`.

### 3.2. `aes_gcm_sw::AesGcmSwProvider`

Реализует `CryptoProvider` для AES-128-GCM и AES-256-GCM через `aes-gcm`
крейт. В тестах изначально я задал «NIST» векторы из памяти — они оказались
выдуманными, 2 теста провалились. Заменил на канонические векторы
**McGrew & Viega "The Galois/Counter Mode of Operation (GCM)" (2005)**:

| Тест | Что покрывает |
|---|---|
| `aes128_gcm_mcgrew_viega_test2` | AES-128, нулевой ключ/IV/PT, пустой AAD — нижний край |
| `aes256_gcm_mcgrew_viega_test14` | AES-256, нулевой ключ/IV/PT, пустой AAD |
| `aes128_gcm_mcgrew_viega_test4` | AES-128, реальные данные, 60-байтный PT, 20-байтный AAD |
| `roundtrip_various_sizes` | 0, 1, 15, 16, 17, 64, 1500, 9000 байт PT |
| `open_fails_on_tampered_ciphertext` | bit-flip → `AuthFailed` |
| `open_fails_on_tampered_aad` | другой AAD → `AuthFailed` |
| `algorithm_mismatch_rejected` | провайдер 128 + ключ 256 → `AlgorithmMismatch` |
| `bad_nonce_length_rejected` | nonce 8 байт вместо 12 → `InvalidNonce` |

### 3.3. `acm-wire::seal/open`

Реализованы функции упаковки и распаковки целого ACM-фрейма:

```text
[ HEADER 12B | NONCE NLen | CIPHERTEXT | TAG 16B ]
```

Header (12 байт: `Magic 'AC' | Ver | Flags | KeyId 4B | Algo | NonceLen | rsvd 2B`)
**целиком используется как AAD** — любая попытка изменить KeyId/Algo/Flags
в transit ломает тег. При этом header остаётся читаемым промежуточному
оборудованию для маршрутизации без расшифровки.

Тесты `acm-wire` (11 шт.):

| Тест | Что проверяет |
|---|---|
| `header_roundtrip` | encode/decode идентичны |
| `header_bad_magic`, `header_bad_version`, `header_unknown_algo` | валидация |
| `seal_open_aes128` | базовый roundtrip AES-128 |
| `seal_open_aes256_various_sizes` | AES-256, размеры 0/1/64/1500/9000 |
| `open_fails_when_header_tampered` | flip байт в `KeyId` → `AuthFailed` |
| `open_fails_when_ciphertext_tampered` | flip байт в CT → `AuthFailed` |
| `open_rejects_truncated_frame` | 5 байт → `Truncated(5)` |
| `open_rejects_mismatched_provider` | seal с AES-128, open с AES-256 → `AlgorithmMismatch` |
| `header_inspectable_plaintext_hidden` | header читается, plaintext не лежит подпоследовательностью в ciphertext |

### 3.4. Результат прогона тестов

    ./dev.sh test
    ...
    running 13 tests   (acm-crypto)       → ok
    running 11 tests   (acm-wire)         → ok
    running  1 test    (acm-dpdk smoke)   → ok
    running  1 test    (acm-ipc proto)    → ok

**Итого 26 тестов проходят, 0 падают.** Время прогона ~24 сек (sequential
build) после холодного старта Docker.

## Итоги сессии (на 2026-05-22, день 2)

* ✅ **Подтверждено:** Rust + RustCrypto `aes-gcm` собирается и работает в
  builder image. Бит-в-бит совпадение с каноническими GCM-векторами.
* ✅ **Подтверждено:** wire-формат с AlgoId работает, header стоит как AAD,
  любая модификация любых полей рамки ловится тегом.
* ❌ **Заблокировано:** прямые бенчмарки `musdk_sam_kat` / `openssl speed` на
  модуле — нужен root, у `user` нет sudo. Ждём пароль root или решение
  владельца модуля добавить `user` в sudoers.
* 📐 **Идея:** наш собственный AES-GCM бинарь, собранный для aarch64, можно
  как `user` залить на модуль и прогнать собственным бенчем. Это даст
  числа для **CPU-пути** (через ARM Crypto Ext), без SAM. SAM-числа —
  только после получения root.

Следующие шаги (если получим root): apt install openssl, openssl speed,
musdk_sam_kat — все три измерения за ~15 минут. Если root не получим
сегодня: соберём бенч-бинарь на aarch64 и прогоним под user.

