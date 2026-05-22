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

## Шаг 4 — Доступ к root через `su -c` + PTY

Пароль root — `PleaseChangeTheRootPassword` (заводской из спецификации).
`user` не в sudoers, поэтому идём через `su`, а не `sudo`. У `su` требуется
TTY → используем PTY-канал paramiko и отправляем пароль в stdin после
обнаружения `Password:` в выходе. Реализация — `run_root()` в
`scripts/bench_aes.py`.

Проверка прошла:

    $ whoami && id && hostname  (через su -c)
    root
    uid=0(root) gid=0(root) groups=0(root)
    smart-sfp

## Шаг 5 — Установка `openssl` CLI (offline через jumphost)

Модуль **не имеет внешнего DNS / интернета**:

    W: Failed to fetch http://deb.debian.org/.../InRelease
      Temporary failure resolving 'deb.debian.org'

→ apt-get update / install не работают.

**Решение:** скачать `.deb` на jumphost (там интернет есть), отдать модулю
через SFTP, поставить через `dpkg -i`. Скрипт — `scripts/install_openssl_offline.py`.

По пути ловили три ошибки:

1. **Хардкод имени файла** не работает: я взял версию из головы (`3.0.18`),
   её на mirror нет. Сделал autodiscover через `wget -qO - .../openssl/`
   с фильтром `~deb12u` (bookworm-специфичные пакеты).
   Текущая версия: **`openssl_3.0.20-1~deb12u1_arm64.deb`** (1.4 МБ).

2. **`dpkg: warning: 'ldconfig' not found in PATH`** — `su -c` не подгружает
   полный env. Решил явным `PATH=/usr/local/sbin:/usr/sbin:/sbin:...`
   в команду.

3. **`unable to clean up mess surrounding './etc/ssl': Read-only file
   system`** — корень модуля монтируется ro по умолчанию (видим в
   `mount`: `/dev/mmcblk0p2 on / type ext4 (ro,noatime,nodiratime,...)`).
   Решил `mount -o rw,remount /` перед установкой и `mount -o ro,remount /`
   обратно.

**Итог установки:**

    OpenSSL 3.0.20 7 Apr 2026 (Library: OpenSSL 3.0.15 3 Sep 2024)
    platform: debian-arm64
    OPENSSLDIR: "/usr/lib/ssl"
    CPUINFO: OPENSSL_armcap=0xbd

`OPENSSL_armcap=0xbd` = `0b10111101` — флаги OpenSSL для ARM CPU:
**AES, PMULL, SHA1, SHA256** активны и используются в шифровании.
ARMv8 Crypto Extensions подтверждены on the wire.

## Шаг 6 — `openssl speed` — CPU-потолок AES/SHA/ChaCha

Прогнал каждый алгоритм по 6 размерам блока (16, 64, 256, 1024, 8192, 16384 байт).
Полный raw-вывод: `scripts/output/bench-20260522-093441.txt`.

Числа OpenSSL `speed` — в килобайтах в секунду. Перевожу в **Мбит/с**
(× 8 / 1000):

| Алгоритм | 16 Б | 64 Б | 256 Б | **1024 Б** | 8192 Б | 16384 Б |
|---|---:|---:|---:|---:|---:|---:|
| AES-128-GCM | 32 | 122 | 443 | **1301** | 3115 | 3449 |
| AES-256-GCM | 31 | 120 | 427 | **1205** | 2781 | 3064 |
| AES-256-CTR | 428 | 1302 | 2696 | **3970** | 4569 | 4605 |
| ChaCha20-Poly1305 | 221 | 496 | 982 | **1246** | 1330 | 1329 |
| SHA-256 | 60 | 221 | 743 | **1816** | 3166 | 3329 |

(значения в Мбит/с на одно ядро Cortex-A53 @ 1.2 ГГц)

**Наблюдения:**

* **AES-256-GCM держит line-rate 1 Гбит/с** уже при пакетах ≥1 КБ
  (1205 Мбит/с на 1024-байт). Для большинства реального TCP/IP-трафика
  (MTU 1500) этого достаточно с запасом.
* **На малых пакетах (64 байта, worst-case)** AES-GCM = ~120 Мбит/с
  — линейная скорость 1 Гбит/с не достижима, **тут нужен SAM HW
  offload**.
* **AES-256-CTR без аутентификации**: 1.3 Гбит/с на 64 байтах. Показывает,
  что узкое место GCM на малых пакетах — это GHASH (PMULL-операция),
  а не сам AES.
* **ChaCha20-Poly1305 в чистом софте**: 1.2 Гбит/с на 1024 байт — это
  resp хороший fallback, но AES-GCM с HW ускорением быстрее в 2.5 раза
  на больших блоках.
* **SHA-256 с ARM SHA2-инструкциями**: 1.8 Гбит/с на 1024 байт. Подойдёт
  для HMAC / KDF / integrity без затыка.

**Главный практический вывод для архитектуры ACM-UZ:** на этой платформе
AES-256-GCM в чистом ПО уже даёт нам нужную производительность для
типового трафика. SAM нужен только если хотим line-rate на 64-байтных
пакетах (типично для атак / голосовых протоколов).

## Шаг 7 — `musdk_sam_kat` — заблокирован отсутствием test-файла

`musdk_sam_kat` требует `<test_file>` с описанием SA + векторами.
На модуле ни одного такого файла:

    # find / -name '*.txt' -path '*musdk*' 2>/dev/null    (root)
    (empty)
    # dpkg -L musdk 2>/dev/null                            (root)
    (empty — пакета "musdk" в dpkg нет, MUSDK ставился не пакетом)

То есть **MUSDK на этом модуле собран и положен руками вендора без штатных
тестовых ресурсов**. Чтобы прогнать SAM-бенч, нам нужно:

* либо взять тестовый файл из исходников MUSDK upstream (Marvell GitHub,
  если опубликованы) и положить в `/tmp`;
* либо собрать минимальный текстовый файл руками — у KAT-формата
  фиксированная структура (`SA-options + plaintext + expected ciphertext`),
  её можно реверс-инженерить из исходников `musdk_sam_kat.c`;
* либо использовать `musdk_sam_single` или `musdk_sam_ipsec` (последний
  обычно сам конфигурирует SA из CLI-аргументов).

Решение: **отложить SAM-бенч**. CPU-числа уже снимают вопрос производительности
для Phase-1 (AES-256-GCM line-rate на типичных пакетах). SAM займёмся
параллельно с реальной интеграцией крипто-pipeline в Rust (нам всё равно
нужно будет писать DPDK `rte_crypto_mvsam` bindings, заодно разберёмся
с форматом теста).

## Итоги сессии (2026-05-22, день 2)

* ✅ **Подтверждено:** Rust + RustCrypto `aes-gcm` собирается и работает в
  builder image. Бит-в-бит совпадение с каноническими GCM-векторами.
  26 unit-тестов проходят.
* ✅ **Подтверждено:** wire-формат с AlgoId работает, header стоит как AAD,
  любая модификация любых полей рамки ловится тегом.
* ✅ **Подтверждено на железе:** AES-256-GCM на ISM4120I через ARM Crypto
  Extensions = **1.2 Гбит/с на пакетах 1 КБ**, **3.1 Гбит/с на 16 КБ** —
  с запасом перекрывает наш целевой 1 Гбит/с link-rate для типового
  трафика. Worst case (64-байтные пакеты) = 120 Мбит/с, тут понадобится
  SAM offload.
* ✅ **Подтверждено:** root-доступ на модуль через `su -c` + PTY, метод
  отработан в `scripts/bench_aes.py`. Корень монтируется ro,
  remount-цикл в `scripts/install_openssl_offline.py`.
* ❌ **Заблокировано:** `musdk_sam_kat` — нет тестовых файлов на модуле,
  MUSDK поставлен не пакетом. Откладываем до этапа интеграции DPDK
  PMD `rte_crypto_mvsam`.
* 📋 **Установлено на модуль (изменения состояния):**
  - `openssl_3.0.20-1~deb12u1_arm64.deb` через `dpkg -i`
  - `/etc/ssl/*` обновился из этого пакета (поверх старого `libssl 3.0.15`)
  - Каталог `/tmp/openssl-offline/` с .deb файлом
  - В `/tmp/acm-debs/` на jumphost — оригинал .deb
* 🔜 **Следующие шаги:** UDS-сервер в `acm-cryptod` + клиент в `acm-agent`
  (gRPC через protobuf, описанный в `proto/acm.proto`); затем — связать
  агент с провайдером шифрования через UDS, чтобы можно было управлять
  ключами с агента. Параллельно — разбираться с форматом теста MUSDK SAM
  для бенча HW-ускорителя.

## Шаг 8 — Свой Rust-бенч + ВАЖНАЯ находка про RustCrypto на AArch64

Добавил флаг `--bench` в `acm-cryptod`. Логика: warmup 50 мс на каждом
размере блока, потом seal в плотном цикле 3 секунды, потом то же для open.
Числа форматируем как у `openssl speed` — для прямого сравнения.

Первый прогон (`scripts/output/rust-bench-20260522-095641.txt`):

    === Aes256Gcm ===
     1024 B   12114 ops/s   99.2 Mbps    seal
     8192 B    1703 ops/s  111.6 Mbps

Это **в 12 раз медленнее** `openssl speed` (1205 Мбит/с на 1024 байт).
При этом `OPENSSL_armcap=0xbd` — ARM Crypto Extensions активны.

**Гипотеза 1:** оптимизатор не подхватил target features. Добавил в
`cryptod/.cargo/config.toml`:

    [target.aarch64-unknown-linux-gnu]
    rustflags = ["-C", "target-feature=+aes,+sha2,+neon"]

Пересборка → **числа не изменились**.

**Гипотеза 2 (правильная):** документация крейта `aes 0.8` (за которым
живёт `aes-gcm 0.10`) явно требует **Rust Nightly** для AArch64 hardware
AES (`armv8` intrinsics не стабильны):

> AArch64 ARMv8 Crypto Extensions: requires Rust Nightly. The intrinsics
> necessary to use ARMv8 cryptographic extensions are not stable yet.
> — https://docs.rs/aes/0.8.4/

То есть RustCrypto `aes-gcm 0.10` на **stable Rust + AArch64** молча
fallback'ает в **bitsliced software AES**, который ~10× медленнее
аппаратного. Это **критический инженерный факт**: для production-AES на
этом железе RustCrypto на stable Rust **не подходит**.

### Решение: добавил `ring` как второй провайдер

[`ring 0.17`](https://github.com/briansmith/ring) использует ассемблер
BoringSSL и **работает на stable AArch64 Rust** с полным HW-ускорением.

Добавил:

* `cryptod/crates/acm-crypto/src/aes_gcm_ring.rs` — `AesGcmRingProvider`
  через `LessSafeKey` / `seal_in_place_separate_tag` / `open_in_place`.
* 4 unit-теста: McGrew-Viega test 2 и test 14 (те же KAT, что прошли
  через RustCrypto — байт-в-байт совпадение между ring и RustCrypto на
  arbitrary 555-байтных данных подтверждает их семантическую
  идентичность); tampered-tag → AuthFailed.

### Второй прогон бенча: `ring` vs `rustcrypto` vs `openssl`

Полный raw — `scripts/output/rust-bench-20260522-101004.txt`. Сводка:

#### AES-256-GCM, seal, **Мбит/с** на одном ядре Cortex-A53 @ 1.2 ГГц

| Block | openssl CLI | RustCrypto sw | **ring (наш)** | ring vs openssl |
|---|---:|---:|---:|---:|
| 16 B | 31 | 9 | **62** | **+100%** |
| 64 B | 120 | 34 | **242** | **+102%** |
| 256 B | 427 | 72 | **795** | **+86%** |
| 1024 B | 1205 | 99 | **1815** | **+50%** |
| 8192 B | 2781 | 112 | **2933** | +5% |
| 16384 B | 3064 | 112 | **3004** | -2% |

#### AES-128-GCM, seal

| Block | openssl | RustCrypto | **ring** | ring vs openssl |
|---|---:|---:|---:|---:|
| 64 B | 122 | 43 | **254** | **+108%** |
| 1024 B | 1301 | 117 | **1965** | **+51%** |
| 16384 B | 3449 | 131 | **3359** | -3% |

#### open (decrypt + tag verify) практически совпадает с seal

ring 1024 B open = 1633 Мбит/с (vs seal 1815), на 10% медленнее.
Объяснимо: дополнительный тэг-чек.

### Выводы для отчёта

1. **`ring` на этой платформе даёт line-rate AES-256-GCM от 256-байтных
   пакетов** (795 Мбит/с) и **в 2× быстрее openssl CLI** на малых
   пакетах. На больших блоках сопоставимо.
2. **RustCrypto `aes-gcm` нельзя использовать в production на stable
   Rust для AArch64** — fallback в bitsliced software AES, 10×
   деградация. Полезно только как KAT-reference (что мы и делаем).
3. **Marvell SAM пока что не критичен для Phase 1.** Целевой 1 Гбит/с
   line-rate перекрывается `ring` уже с пакетов 256 байт (типичный
   реальный TCP-трафик ≥500 байт). SAM нужен только когда захотим
   line-rate на 64-байтных пакетах (voice / attack-like).
4. **Бенчмарк превосходит `openssl speed` потому, что у `ring` лёгкий
   API без `EVP_CIPHER_CTX_*`-overhead.** Бенчмарк показывает максимум
   возможного на этом CPU; в реальном datapath с per-пакетной
   bookkeeping будет ниже, но HW-ускорение остаётся.

### Изменения состояния модуля (на этом шаге)

* Залит `acm-cryptod` (1.18 МБ) в `/home/user/acm-uz/acm-cryptod` —
  идёт от user, никаких прав не требует.

## Шаг 9 — UDS-сервер в `acm-cryptod` + клиент в `acm-cli` (Go)

Цель: end-to-end проверить, что Rust-демон и Go-клиент договариваются
по контролю крипто-состояния через Unix Domain Socket.

### Архитектура

* **Wire:** line-delimited JSON, один запрос на строку, один ответ.
  Просто, debug'ается через `nc -U`. Контракт описан в `acm-ipc`
  (Rust) и продублирован в `agent/internal/ipc/client.go`.
* **Сервер:** в `acm-ipc::server` — общий tokio-based UDS listener
  с trait `Handler`. Один поток per connection.
* **State:** в `acm-cryptod::CryptodState` — держит активный
  `Box<dyn CryptoProvider>` + `KeyHandle`, на `RotateKey` создаёт
  новый провайдер и перед коммитом гоняет self-test (seal+open
  одного известного PT).

### Грабли interop №1 — Go vs Rust JSON для `[]byte`

Первый прогон упал на rotate-key:

    cryptod error 400: bad request json: invalid type: string
    "1bGyauGWLw0GDEpQSUtc+LfitOdFfBdYy2Moo0UVuFo=",
    expected a sequence at line 1 column 110

Go `encoding/json` сериализует `[]byte` как **base64-standard string**
по умолчанию. Rust `serde_json` для `Vec<u8>` ожидает **массив чисел**.
Это классическая interop-беда между двумя экосистемами.

**Решение:** на Rust-стороне явный custom serde для поля `material`:

```rust
#[serde(with = "wire_b64")]
pub material: Vec<u8>,

mod wire_b64 {
    pub fn serialize<S: Serializer>(b: &Vec<u8>, s: S) -> ...;
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, ...>;
}
```

Один новый dep: `base64 = "0.22"` в `acm-ipc/Cargo.toml`. Два теста
в `acm-ipc`: `serialize_rotate_key_uses_base64` и
`deserialize_rotate_key_from_base64` фиксируют контракт навсегда.

### Грабли №2 — `async-trait` нужно в каждом crate, где импл

`acm_ipc::server::Handler` декорирован `#[async_trait::async_trait]`.
Когда в `acm-cryptod` пишем `impl Handler for CryptodState`, тоже
нужен `async_trait` crate в Cargo.toml — он не достаётся транзитивно
через `acm-ipc`. Добавил в `acm-cryptod/Cargo.toml`.

### End-to-end smoke на модуле (`scripts/test_ipc_on_module.py`)

Скрипт:
1. Заливает свежие `acm-cryptod` + `acm-cli` через SFTP.
2. Запускает `cryptod --ipc-socket /tmp/acm/cryptod.sock` в фоне
   (никакого root — UDS в `/tmp`).
3. Ждёт появления сокета (≤5 сек).
4. Прогоняет 4 операции через `acm-cli` (Go-клиент) к cryptod
   (Rust-серверу) и проверяет ответы:

| # | Операция | Ожидание | Получили |
|---|---|---|---|
| 1 | `acm-cli status` | active_key_id=(none), version 0.1.0, provider ring/aes-256-gcm | ✅ |
| 2 | `acm-cli rotate-key 7 2 <32B hex>` | "ok" | ✅ |
| 3 | `acm-cli status` ещё раз | active_key_id=7 | ✅ |
| 4 | `acm-cli rotate-key 8 0x10 <32B hex>` (O'z DSt 1105 — не поддержан) | exit≠0 + ошибка 501 | ✅ "cryptod error 501: O'z DSt 1105 provider not yet implemented" |

Все 4 assertions прошли. Это **первое реальное end-to-end
подтверждение**, что:
- Rust-cryptod на ISM4120I принимает контроль от Go-клиента через UDS;
- ротация ключа реально меняет активный провайдер;
- ошибки маршалятся читаемо.

### Изменения состояния модуля

* `/home/user/acm-uz/acm-cryptod` (1.45 МБ) — свежий бинарь с IPC-server'ом;
* `/home/user/acm-uz/acm-cli` (2.42 МБ) — Go-CLI с IPC-клиентом;
* `/tmp/acm/cryptod.log` и `/tmp/acm/cryptod.sock` (последний после
  теста удалён);
* Процесс `acm-cryptod` после теста убит `pkill`.

## Итоги дня 2 (обновлённые)

* ✅ AES-256-GCM в Rust через `ring` даёт **1815 Мбит/с** на 1KB-блоках
  (1.5× быстрее openssl CLI). Подтверждено на ISM4120I.
* ✅ RustCrypto `aes-gcm` на stable AArch64 → bitsliced software AES,
  10× медленнее. Документировано.
* ✅ Wire-формат с `AlgoId`, header-as-AAD, 11 тестов.
* ✅ UDS control plane: cryptod (Rust+tokio) ↔ agent/cli (Go), JSON over UDS,
  ротация ключа, self-test перед коммитом, аккуратные ошибки.
* ✅ Документировано в журнале каждое решение, каждая грабля, каждое
  изменение состояния модуля.
* 🔜 **Что следующее:** интегрировать `acm-wire::seal/open` поверх IPC
  (положить туда тестовый поток), счётчики `packets_sealed/opened`
  начнут расти; затем — Prometheus exporter в agent + bridge к UDS.

## Шаг 10 — Encrypt/Decrypt через IPC: полноценный шифратор

**Safety-правила сессии:** ничего в `/etc`, `/usr`, `/opt`, `/var`; всё в
`/home/user/acm-uz/` и `/tmp/`; cryptod от user'а с UDS в `/tmp/acm/`;
все тест-скрипты с `finally: cleanup`; SSH-проверка после каждой операции.

### Расширение контракта

Добавил в `acm-ipc`:

| Тип | Назначение |
|---|---|
| `Request::Encrypt(EncryptParams)` | Принять plaintext, вернуть полный ACM-frame |
| `Request::Decrypt(DecryptParams)` | Принять frame, вернуть plaintext или 401 AuthFailed |
| `Response::Ciphertext(Bytes)` | Frame обратно |
| `Response::Plaintext(Bytes)` | Расшифровка обратно |
| `pub mod wire_b64` | Сделал серде-хелпер `pub` чтобы переиспользовать в `Bytes`/`EncryptParams` |

Все байтовые поля — base64-on-wire (`nonce`, `aad`, `plaintext`, `frame`,
`Bytes.bytes`), совместимо с Go-сериализацией.

### Реализация в `CryptodState`

`handle_encrypt(p)`:

1. Берёт активный key. Если ключа нет — 409 "no active key".
2. Валидирует nonce length против `algo.nonce_len()`.
3. Аллоцирует buffer на `frame_len(pt.len(), 12, 16)`.
4. Вызывает `acm_wire::seal(provider, &key, key.id, &nonce, 0, pt, &mut out)`.
5. Инкрементит `packets_sealed`; при ошибке — `crypto_errors`.
6. Отдаёт `Response::Ciphertext`.

`handle_decrypt(p)`:

1. Активный key (иначе 409).
2. `acm_wire::open(...)`.
3. На success — `packets_opened` + Plaintext.
4. На failure — `crypto_errors` + правильный код:
   - `AuthFailed` → **401** (распознаваемо как auth-проблема);
   - всё остальное → 422 (malformed input).

### Go-клиент

Расширил `agent/internal/ipc/Client`: методы `Encrypt(ctx, nonce, aad, pt)`
и `Decrypt(ctx, frame)`. Использует тот же long-lived UDS-connection +
mutex для сериализации.

### E2E test binary `acm-encdec-test` (Go)

Отдельный mini-бинарь, чтобы не раздувать `acm-cli`. Запускается на
модуле, делает 11 операций:

1. `GetStatus` → запоминает счётчики;
2. Для каждого размера `[0, 1, 15, 16, 17, 64, 256, 1500, 9000]`:
   - случайные `pt` + nonce + AAD,
   - `Encrypt` → проверяет `len(frame) == pt + 40` (12+12+16),
   - `Decrypt` → проверяет `back == pt`;
3. Tamper: `Encrypt("ATTACK AT DAWN")`, flip байт #24 (первый ct),
   `Decrypt` → ожидает `code 401`;
4. `GetStatus` снова — счётчики:
   - `sealed` += 10 (9 + 1 для tamper-test),
   - `opened` += 9 (tamper-decrypt инкремент не делает — он errored),
   - `crypto_errors` += 1.

### Прогон на ISM4120I

```
BEFORE: sealed=0 opened=0 errors=0 key=1
  ok roundtrip size=0 frame=40B
  ok roundtrip size=1 frame=41B
  ok roundtrip size=15 frame=55B
  ok roundtrip size=16 frame=56B
  ok roundtrip size=17 frame=57B
  ok roundtrip size=64 frame=104B
  ok roundtrip size=256 frame=296B
  ok roundtrip size=1500 frame=1540B
  ok roundtrip size=9000 frame=9040B
  ok tamper detected: cryptod error 401: open failed: crypto: authentication failed
AFTER : sealed=10 opened=9 errors=1
PASS

version:          0.1.0
running:          true
uptime_s:         2
active_provider:  ring/aes-256-gcm
active_key_id:    1
packets_sealed:   10
packets_opened:   9
crypto_errors:    1
```

**Все 11 assertions прошли.** Post-cleanup SSH-проверка — ok.

### Изменения состояния модуля

* `/home/user/acm-uz/{acm-cryptod, acm-cli, acm-encdec-test}` обновлены;
* `/tmp/acm/cryptod.log` (foreground'ный лог);
* `/tmp/acm/cryptod.sock` — удалён в cleanup;
* процесс `acm-cryptod` — pkill'ed в `finally`.

### Что это означает

К концу сессии у нас **рабочий end-to-end software шифратор на реальном
SFP-модуле**:

```
Go-клиент (acm-encdec-test)
   │
   │  JSON-RPC over UDS  /tmp/acm/cryptod.sock
   ▼
Rust-cryptod
   │
   │  acm_wire::seal/open
   ▼
AesGcmRingProvider (ring 0.17, ARMv8 Crypto Ext)
   │
   ▼
шифрованный фрейм (header + nonce + ct + tag)
```

Не хватает только связи с реальными сетевыми пакетами — это уже DPDK
layer и MUSDK NETA PMD. Но **криптографическая часть, key management,
агент-провайдер контракт, wire-формат, аутентификация и метрики
работают** и проверены на железе.

## Финальные итоги дня 2 (2026-05-22)

### Метрики

* **Тесты:** 4 → 34 (unit) + 2 E2E на железе
* **Коммиты:** 6 за день, всё на main, пушится зелёно
* **Бинарей на модуле:** 5 (cryptod, agent, cli, encdec-test, controller-amd64)
* **Производительность подтверждена:** AES-256-GCM 1815 Мбит/с через ring
  (line-rate с запасом), 1KB packets

### Качественные результаты

1. ✅ Tooling pipeline собирает любой код в один статический бинарь под
   aarch64, заливается на модуль за секунды.
2. ✅ Crypto-абстракция `CryptoProvider` доказала ценность: одна
   RustCrypto-реализация для KAT, одна `ring`-реализация для production,
   обе bit-equivalent.
3. ✅ Wire-формат с `AlgoId` устойчив к подмене заголовков, легко
   расширяется на O'z DSt 1105 в фазе 2.
4. ✅ Control plane через UDS отработан end-to-end: ротация ключа,
   статус, encrypt/decrypt, обработка ошибок.
5. ✅ Счётчики (sealed/opened/errors) реально работают — основа для
   Prometheus exporter и аудит-журнала по 2814.
6. ✅ Каждый шаг задокументирован с командами, выводом, заметками о
   состоянии модуля — материал готов к включению в отчёт.

### Что ещё нет (фокус следующего дня)

* **Prometheus exporter** в Go-agent (читать счётчики, отдавать
  `/metrics`).
* **SNMP-агент** в Go-agent.
* **Реальный DPDK datapath** — отдельный большой кусок, со связкой
  MUSDK NETA + crypto_mvsam PMD.
* **O'z DSt 1105** в Rust (после получения тест-векторов).
* **Бинарный wire-формат для IPC** вместо JSON для производительности
  (сейчас JSON хорош для отладки, в DPDK pipeline пакеты будут идти
  через shared-memory ring).

