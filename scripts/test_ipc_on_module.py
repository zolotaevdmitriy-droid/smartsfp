"""
End-to-end проверка UDS IPC между cryptod и cli на ISM4120I:
  1. Залить свежие acm-cryptod, acm-cli в /home/user/acm-uz/.
  2. Запустить cryptod с UDS-сокетом в /tmp/acm/cryptod.sock (root не нужен).
  3. Подождать пока сокет появится.
  4. Прогнать `acm-cli status`, ожидаем StatusReport.
  5. Прогнать `acm-cli rotate-key 7 2 <32 байт ключа hex>`, ожидаем "ok".
  6. Снова `status` — active_key_id должен стать 7.
  7. Попробовать `rotate-key 8 16 <32 байта>` (O'z DSt 1105) — ожидаем
     осмысленную ошибку 501 "not yet implemented".
  8. Прибить cryptod.

Никаких изменений системы — всё в /tmp и /home/user.
"""
import os
import sys
import time
import secrets
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

DIST = os.path.join(os.path.dirname(__file__), "..", "dist")
REMOTE_DIR = "/home/user/acm-uz"
SOCKET = "/tmp/acm/cryptod.sock"


def connect():
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
    return jc, tc


def run(tc, cmd, timeout=30):
    stdin, stdout, stderr = tc.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    return rc, out, err


def main():
    print("[*] connect"); jc, tc = connect(); print("[+] ready")

    # 1. Залить свежие бинари
    sftp = tc.open_sftp()
    run(tc, f"mkdir -p {REMOTE_DIR} && rm -f {REMOTE_DIR}/acm-*")
    for name in ("acm-cryptod", "acm-cli"):
        local = os.path.join(DIST, name)
        remote = f"{REMOTE_DIR}/{name}"
        size = os.path.getsize(local)
        print(f"[*] scp {name} ({size} B)")
        sftp.put(local, remote); sftp.chmod(remote, 0o755)
    sftp.close()

    # 2. Подготовим каталог под сокет, убьём старый cryptod если есть
    run(tc, "mkdir -p /tmp/acm && rm -f /tmp/acm/cryptod.sock && pkill -f acm-cryptod || true")
    time.sleep(0.5)

    # 3. Запустим cryptod в фоне
    print("[*] start cryptod (background)")
    bg_cmd = (f"nohup {REMOTE_DIR}/acm-cryptod "
              f"--ipc-socket {SOCKET} "
              f"> /tmp/acm/cryptod.log 2>&1 & echo pid=$!")
    rc, out, err = run(tc, bg_cmd, timeout=10)
    print(f"    {out.strip()}")

    # 4. Дождёмся сокета (до 5 сек)
    deadline = time.time() + 5
    sock_ok = False
    while time.time() < deadline:
        rc, out, _ = run(tc, f"test -S {SOCKET} && echo OK || echo NOT_YET")
        if "OK" in out:
            sock_ok = True; break
        time.sleep(0.3)
    if not sock_ok:
        print("[!] socket did not appear in 5s; cryptod log:")
        _, log, _ = run(tc, "cat /tmp/acm/cryptod.log")
        print(log)
        sys.exit(2)
    print(f"[+] socket ready: {SOCKET}")

    # 5. status #1
    print("\n--- acm-cli status (before rotate-key) ---")
    rc, out, err = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} status")
    print(f"exit={rc}")
    print(out, end="")
    if err.strip(): print("stderr:", err)
    assert rc == 0
    assert "version:" in out
    assert "active_key_id:    (none)" in out

    # 6. rotate-key — AES-256-GCM, 32 байта random
    keyhex = secrets.token_hex(32)
    print(f"\n--- acm-cli rotate-key 7 2 <{keyhex[:16]}...> ---")
    rc, out, err = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} rotate-key 7 2 {keyhex}")
    print(f"exit={rc} stdout={out.strip()!r}")
    if err.strip(): print("stderr:", err)
    assert rc == 0
    assert out.strip() == "ok"

    # 7. status #2 — должен show active_key_id=7
    print("\n--- acm-cli status (after rotate-key) ---")
    rc, out, err = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} status")
    print(f"exit={rc}")
    print(out, end="")
    assert rc == 0
    assert "active_key_id:    7" in out
    assert "ring/aes-256-gcm" in out

    # 8. rotate-key с не поддержанным алгоритмом → ожидаем error 501
    badkey = secrets.token_hex(32)
    print(f"\n--- acm-cli rotate-key 8 0x10 <ozdst1105 key> (expect 501) ---")
    rc, out, err = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} rotate-key 8 16 {badkey}")
    print(f"exit={rc} stdout={out.strip()!r} stderr={err.strip()!r}")
    assert rc != 0
    assert "501" in err
    assert "not yet implemented" in err.lower()

    # 9. cleanup
    print("\n[*] kill cryptod")
    run(tc, "pkill -f acm-cryptod || true; rm -f " + SOCKET)
    print("\n[+] all assertions passed")

    tc.close(); jc.close()


if __name__ == "__main__":
    try:
        main()
    except AssertionError as e:
        print(f"\n[!] ASSERTION FAILED: {e}")
        sys.exit(3)
    except Exception as e:
        print(f"\n[!] {type(e).__name__}: {e}")
        sys.exit(1)
