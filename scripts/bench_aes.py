"""
Бенчмарки AES на ISM4120I.

Логин: ssh user@192.168.0.99 (через jumphost), потом `su -c '...'` с root
паролем (PleaseChangeTheRootPassword). У `su` требуется TTY → используем
PTY-канал paramiko и подаём пароль в stdin.

Запуск:
    python scripts/bench_aes.py

Полный raw-лог пишется в scripts/output/bench-YYYYMMDD-HHMMSS.txt
(каталог output/ в .gitignore).
"""
import os
import re
import sys
import time
import shlex
import datetime as dt
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"
ROOT_PASS = "PleaseChangeTheRootPassword"

OUT_DIR = os.path.join(os.path.dirname(__file__), "output")
os.makedirs(OUT_DIR, exist_ok=True)
STAMP = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
LOG_PATH = os.path.join(OUT_DIR, f"bench-{STAMP}.txt")


# ----------------------------------------------------------------
# SSH plumbing
# ----------------------------------------------------------------
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


def run_user(tc, cmd, timeout=60):
    """Plain exec_command under `user`. Returns (rc, stdout, stderr)."""
    stdin, stdout, stderr = tc.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    return rc, out, err


def run_root(tc, cmd, timeout=120):
    """
    Execute `cmd` as root via `su -c`. Returns (rc, output).

    Uses PTY because Debian 12's `su` requires a TTY for password input.
    After password prompt, everything up to and including the prompt line
    is stripped from the output. stderr is merged into stdout.
    """
    su_line = f"su -c {shlex.quote(cmd)}"
    chan = tc.get_transport().open_session()
    chan.settimeout(timeout + 5)
    chan.get_pty(term="dumb", width=200, height=50)
    chan.exec_command(su_line)

    buf = bytearray()
    sent_pw = False
    start = time.time()
    while True:
        # Read available data without blocking; tiny sleep otherwise.
        if chan.recv_ready():
            data = chan.recv(16384)
            if data:
                buf.extend(data)
        # Detect password prompt the first time and send the password.
        if not sent_pw and b"assword:" in buf:
            chan.send(ROOT_PASS + "\n")
            sent_pw = True
        # Detect end-of-command (exit status + no more data buffered).
        if chan.exit_status_ready() and not chan.recv_ready():
            # One more drain pass.
            time.sleep(0.05)
            while chan.recv_ready():
                buf.extend(chan.recv(16384))
            break
        if time.time() - start > timeout:
            chan.close()
            raise TimeoutError(f"root cmd timed out after {timeout}s: {cmd!r}")
        time.sleep(0.03)

    rc = chan.recv_exit_status()
    text = buf.decode(errors="replace")
    # Strip "Password:" prompt line — everything up to first newline after
    # the literal "Password:" word.
    text = re.sub(r"(?s)^.*?[Pp]assword:[^\r\n]*\r?\n", "", text, count=1)
    # If su itself failed (wrong password), nothing was stripped — but
    # then we'd see "su: Authentication failure" in the output. Trust the
    # caller to inspect rc.
    return rc, text


# ----------------------------------------------------------------
# Logger
# ----------------------------------------------------------------
def section(title):
    line = "=" * 78
    return f"\n{line}\n  {title}\n{line}\n"


class Log:
    def __init__(self, path):
        self.path = path
        self.fh = open(path, "w", encoding="utf-8")

    def write(self, block):
        self.fh.write(block)
        self.fh.flush()

    def close(self):
        self.fh.close()


def record(log, label, cmd, rc, out, err="", dur=None):
    head = f"{label}"
    if dur is not None:
        head += f"  ({dur:.1f}s)"
    blocks = [section(head), f"$ {cmd}", f"exit={rc}", ""]
    if out:
        blocks.append("--- stdout ---")
        blocks.append(out.rstrip())
    if err:
        blocks.append("--- stderr ---")
        blocks.append(err.rstrip())
    log.write("\n".join(blocks) + "\n")


# ----------------------------------------------------------------
# Main
# ----------------------------------------------------------------
def main():
    print(f"[*] connect…")
    jc, tc = connect()
    print(f"[+] {TARGET_USER}@{TARGET_HOST}")
    log = Log(LOG_PATH)

    def step_user(label, cmd, timeout=60, echo=15):
        print(f"[*] {label}")
        t0 = time.time()
        rc, out, err = run_user(tc, cmd, timeout=timeout)
        dur = time.time() - t0
        record(log, label, cmd, rc, out, err, dur)
        lines = (out or err).rstrip().splitlines()
        print(f"    rc={rc} time={dur:.1f}s")
        for ln in lines[:echo]:
            print(f"    | {ln}")
        if len(lines) > echo:
            print(f"    | ... ({len(lines)-echo} more lines in log)")
        return rc, out, err

    def step_root(label, cmd, timeout=180, echo=15):
        print(f"[*] {label}  (root)")
        t0 = time.time()
        try:
            rc, out = run_root(tc, cmd, timeout=timeout)
        except TimeoutError as e:
            rc, out = -1, str(e)
        dur = time.time() - t0
        record(log, label + "  [root]", cmd, rc, out, dur=dur)
        lines = out.rstrip().splitlines()
        print(f"    rc={rc} time={dur:.1f}s")
        for ln in lines[:echo]:
            print(f"    | {ln}")
        if len(lines) > echo:
            print(f"    | ... ({len(lines)-echo} more lines in log)")
        return rc, out

    # ----------------------------------------------------------------
    # 0. Sanity: user identity + root via su works
    # ----------------------------------------------------------------
    step_user("sanity / whoami as user", "whoami; id")
    rc, out = step_root("sanity / whoami as root", "whoami && id && hostname")
    if "root" not in out:
        print("\n[!] su to root did not succeed. Output above.")
        log.close()
        sys.exit(2)

    # ----------------------------------------------------------------
    # 1. Install openssl CLI if missing (apt update + apt install)
    # ----------------------------------------------------------------
    rc_u, out_u, _ = run_user(tc, "command -v openssl")
    if rc_u != 0:
        step_root("apt-get update",
                  "DEBIAN_FRONTEND=noninteractive apt-get update -qq 2>&1",
                  timeout=240, echo=5)
        step_root("apt-get install openssl",
                  "DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssl 2>&1",
                  timeout=240, echo=10)
    else:
        log.write(section("openssl CLI already installed") + out_u + "\n")
        print("[+] openssl CLI already present")

    step_user("openssl version -a", "openssl version -a")

    # ----------------------------------------------------------------
    # 2. openssl speed — снимаем CPU-потолок с ARM Crypto Extensions
    # ----------------------------------------------------------------
    step_user("openssl speed aes-128-gcm",
              "openssl speed -elapsed -evp aes-128-gcm 2>&1",
              timeout=180, echo=20)
    step_user("openssl speed aes-256-gcm",
              "openssl speed -elapsed -evp aes-256-gcm 2>&1",
              timeout=180, echo=20)
    step_user("openssl speed aes-256-ctr",
              "openssl speed -elapsed -evp aes-256-ctr 2>&1",
              timeout=180, echo=20)
    step_user("openssl speed chacha20-poly1305",
              "openssl speed -elapsed -evp chacha20-poly1305 2>&1",
              timeout=180, echo=20)
    step_user("openssl speed sha256",
              "openssl speed -elapsed -evp sha256 2>&1",
              timeout=180, echo=20)

    # ----------------------------------------------------------------
    # 3. musdk_sam_kat — нужен root + CMA + тестовый файл
    # ----------------------------------------------------------------
    # Сначала найдём, где живут тестовые файлы.
    step_root("locate musdk test files",
              "find / -xdev -name '*.txt' -path '*musdk*' 2>/dev/null; "
              "find / -xdev -name '*sam*test*' 2>/dev/null; "
              "find /usr/share -xdev -type d 2>/dev/null | grep -iE 'musdk|sam' | head -5; "
              "ls /usr/share/musdk 2>/dev/null; "
              "dpkg -L musdk 2>/dev/null | head -40; "
              "dpkg -L musdk-dev 2>/dev/null | head -10; "
              "dpkg -l | grep -i musdk")

    # Базовый kat-запуск с дефолтным CIO.
    step_root("musdk_sam_kat --help",
              "musdk_sam_kat --help 2>&1 || musdk_sam_kat -h 2>&1",
              echo=30)

    # Попробуем запустить — без тест-файла он подскажет, какие принимает.
    step_root("musdk_sam_kat probe",
              "musdk_sam_kat cio-0:0 2>&1 | head -30 || true")

    log.close()
    tc.close()
    jc.close()
    print(f"\n[+] full log: {LOG_PATH}")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}")
        sys.exit(1)
