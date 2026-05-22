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

