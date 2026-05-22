"""
Залить свежий acm-cryptod на ISM4120I и прогнать --bench.
Сравним числа с openssl speed.
"""
import os, sys, time, datetime as dt
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

DIST = os.path.join(os.path.dirname(__file__), "..", "dist")
OUT_DIR = os.path.join(os.path.dirname(__file__), "output")
os.makedirs(OUT_DIR, exist_ok=True)
STAMP = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
LOG = os.path.join(OUT_DIR, f"rust-bench-{STAMP}.txt")

REMOTE_DIR = "/home/user/acm-uz"
LOCAL_BIN  = os.path.join(DIST, "acm-cryptod")
REMOTE_BIN = f"{REMOTE_DIR}/acm-cryptod"


def main():
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
               timeout=20, allow_agent=False, look_for_keys=False)
    sock = jc.get_transport().open_channel("direct-tcpip",
        (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0), timeout=20)
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER,
               password=TARGET_PASS, sock=sock, timeout=20,
               allow_agent=False, look_for_keys=False)
    print(f"[+] {TARGET_USER}@{TARGET_HOST}")

    # 1. Заливаем свежий бинарь
    sftp = tc.open_sftp()
    tc.exec_command(f"mkdir -p {REMOTE_DIR}")
    sz = os.path.getsize(LOCAL_BIN)
    print(f"[*] scp acm-cryptod ({sz} B) -> {REMOTE_BIN}")
    sftp.put(LOCAL_BIN, REMOTE_BIN)
    sftp.chmod(REMOTE_BIN, 0o755)
    sftp.close()

    # 2. Версия
    print("[*] version check")
    _, stdout, _ = tc.exec_command(f"{REMOTE_BIN} --version")
    print("    | " + stdout.read().decode().strip())

    # 3. Прогон bench
    print(f"[*] running --bench (3s per size, ~36s total per algo, 2 algos)…")
    _, stdout, stderr = tc.exec_command(
        f"{REMOTE_BIN} --bench --bench-seconds 3", timeout=180)
    out = stdout.read().decode()
    err = stderr.read().decode()
    rc = stdout.channel.recv_exit_status()
    print(f"    rc={rc}")
    print()
    print(out)
    if err.strip():
        print("--- stderr ---")
        print(err)

    # 4. Сохраняем raw
    with open(LOG, "w", encoding="utf-8") as f:
        f.write(f"acm-cryptod --bench on ISM4120I @ {dt.datetime.now().isoformat()}\n")
        f.write("=" * 78 + "\n")
        f.write(out)
        if err.strip():
            f.write("\n--- stderr ---\n" + err)
    print(f"\n[+] saved {LOG}")
    tc.close(); jc.close()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}"); sys.exit(1)
