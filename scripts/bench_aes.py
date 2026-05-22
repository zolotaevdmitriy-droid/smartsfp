"""
Прогнать бенчмарки AES на ISM4120I через jumphost.
Использует sudo с паролем (тот же что и для user).

Что делает:
  1. Проверяет / устанавливает openssl CLI (apt install — единственное
     изменение состояния модуля).
  2. openssl speed -evp aes-128-gcm / aes-256-gcm / chacha20-poly1305
     — потолок программного AES через ARM Crypto Extensions.
  3. musdk_sam_kat для AES-128-GCM с разными размерами буфера — потолок
     HW-ускорителя SAM.
  4. Кладёт raw-вывод обоих в scripts/output/bench-YYYYMMDD-*.txt
     (в .gitignore).

Скрипт сам ничего не парсит — это делает человек / отдельный скрипт.
"""
import os
import sys
import time
import datetime as dt
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

OUT_DIR = os.path.join(os.path.dirname(__file__), "output")
os.makedirs(OUT_DIR, exist_ok=True)
STAMP = dt.datetime.now().strftime("%Y%m%d-%H%M%S")


def connect():
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
               timeout=20, allow_agent=False, look_for_keys=False)
    sock = jc.get_transport().open_channel(
        "direct-tcpip", (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0), timeout=20)
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER,
               password=TARGET_PASS, sock=sock, timeout=20,
               allow_agent=False, look_for_keys=False)
    return jc, tc


def run(tc, cmd, timeout=60, sudo=False):
    """Run a command, return (rc, stdout, stderr). For sudo, password is
    fed via stdin (sudo -S)."""
    full = f"sudo -S -p '' bash -c {shell_quote(cmd)}" if sudo else cmd
    stdin, stdout, stderr = tc.exec_command(full, timeout=timeout)
    if sudo:
        stdin.write(TARGET_PASS + "\n")
        stdin.flush()
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    return rc, out, err


def shell_quote(s):
    return "'" + s.replace("'", "'\"'\"'") + "'"


def section(title):
    line = "=" * 78
    return f"\n{line}\n  {title}\n{line}\n"


def dump(title, cmd, rc, out, err):
    blocks = [section(title), f"$ {cmd}", f"exit={rc}", ""]
    if out:
        blocks.append("--- stdout ---")
        blocks.append(out.rstrip())
    if err:
        blocks.append("--- stderr ---")
        blocks.append(err.rstrip())
    return "\n".join(blocks) + "\n"


def main():
    print(f"[*] connect…")
    jc, tc = connect()
    print(f"[+] {TARGET_USER}@{TARGET_HOST}")

    log_path = os.path.join(OUT_DIR, f"bench-{STAMP}.txt")
    log = open(log_path, "w", encoding="utf-8")

    def step(title, cmd, *, sudo=False, timeout=600, echo_lines=20):
        print(f"[*] {title}")
        t0 = time.time()
        rc, out, err = run(tc, cmd, timeout=timeout, sudo=sudo)
        dur = time.time() - t0
        block = dump(f"{title}  ({dur:.1f}s)", cmd, rc, out, err)
        log.write(block)
        log.flush()
        # Эхо в консоль (короткое)
        printable = (out or err).rstrip().splitlines()
        print(f"    rc={rc} time={dur:.1f}s")
        for ln in printable[:echo_lines]:
            print(f"    | {ln}")
        if len(printable) > echo_lines:
            print(f"    | ... ({len(printable)-echo_lines} more lines in log)")
        return rc, out, err

    # ----------------------------------------------------------------
    # 0. Sanity: identify module + sudo works
    # ----------------------------------------------------------------
    step("uname / cpuinfo", "uname -a; grep -m1 Features /proc/cpuinfo; free -m | head -2")
    step("whoami without sudo", "whoami")
    step("whoami via sudo", "whoami", sudo=True, echo_lines=3)

    # ----------------------------------------------------------------
    # 1. Install openssl CLI (if missing)
    # ----------------------------------------------------------------
    rc, out, _ = run(tc, "command -v openssl")
    if rc != 0:
        step("apt update", "DEBIAN_FRONTEND=noninteractive apt-get update -qq",
             sudo=True, timeout=180, echo_lines=5)
        step("apt install openssl", "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssl",
             sudo=True, timeout=180, echo_lines=10)
    else:
        log.write(section("openssl CLI already present") + out + "\n")
        print("[+] openssl already installed")

    step("openssl version", "openssl version -a")

    # ----------------------------------------------------------------
    # 2. openssl speed — пробежать ключевые AEAD / cipher
    # ----------------------------------------------------------------
    # `-evp` гарантирует использование EVP path → подхватит ARM Crypto Ext.
    # `-elapsed` — реальная стенка, не CPU time.
    # Размеры буферов: 64, 256, 1024, 8192, 16384 байт.
    step("openssl speed aes-128-gcm",
         "openssl speed -elapsed -evp aes-128-gcm",
         timeout=120, echo_lines=15)
    step("openssl speed aes-256-gcm",
         "openssl speed -elapsed -evp aes-256-gcm",
         timeout=120, echo_lines=15)
    step("openssl speed aes-256-ctr",
         "openssl speed -elapsed -evp aes-256-ctr",
         timeout=120, echo_lines=15)
    step("openssl speed chacha20-poly1305",
         "openssl speed -elapsed -evp chacha20-poly1305",
         timeout=120, echo_lines=15)
    step("openssl speed sha256",
         "openssl speed -elapsed -evp sha256",
         timeout=120, echo_lines=15)

    # ----------------------------------------------------------------
    # 3. musdk_sam_kat — нужен root и тестовый файл с векторами
    # ----------------------------------------------------------------
    # Сначала найдём, какие тестовые файлы есть.
    step("find musdk test files",
         "find / -xdev -name '*.txt' -path '*musdk*' 2>/dev/null; "
         "find / -xdev -name '*.txt' -path '*sam*' 2>/dev/null; "
         "find /usr/share -xdev -type d -name '*musdk*' 2>/dev/null; "
         "ls -la /usr/share/musdk 2>/dev/null; "
         "dpkg -L musdk 2>/dev/null | head -30")

    # KAT обычно поставляется в виде <name>.txt — пробуем стандартное имя.
    # Если найдём через find выше — выберем оттуда вручную.
    step("musdk_sam_kat run (default test)",
         "musdk_sam_kat cio-0:0 /usr/share/musdk/aes_128_cbc.txt -c 1000 2>&1 || "
         "musdk_sam_kat cio-0:0 aes_128_cbc.txt -c 1000 2>&1 || true",
         sudo=True, timeout=60, echo_lines=20)

    log.close()
    tc.close(); jc.close()
    print(f"\n[+] full log saved to: {log_path}")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}")
        sys.exit(1)
