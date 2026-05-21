"""
Залить собранные бинари на ISM4120I через jumphost, запустить acm-cryptod
и acm-agent один раз для проверки.
"""
import os
import sys
import paramiko

JUMP_HOST = "178.104.223.171"
JUMP_USER = "root"
JUMP_PASS = "Cfvgbxn38"

TARGET_HOST = "192.168.0.99"
TARGET_PORT = 22
TARGET_USER = "user"
TARGET_PASS = "PleaseChangeTheUserPassword"

DIST = os.path.join(os.path.dirname(__file__), "..", "dist")
BINARIES = ["acm-cryptod", "acm-agent", "acm-cli"]
REMOTE_DIR = "/home/user/acm-uz"


def open_target():
    print(f"[*] Jumphost {JUMP_USER}@{JUMP_HOST} ...")
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
    print(f"[+] {TARGET_USER}@{TARGET_HOST} ready")
    return jc, tc


def run(tc, cmd, expect_zero=True):
    stdin, stdout, stderr = tc.exec_command(cmd, timeout=30)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    print(f"$ {cmd}")
    if out.strip():
        print(out.rstrip())
    if err.strip():
        print(f"(stderr) {err.rstrip()}")
    if expect_zero and rc != 0:
        print(f"[!] command exited with rc={rc}")
    return rc, out, err


def main():
    jc, tc = open_target()

    # 1. Подготовка каталога
    run(tc, f"mkdir -p {REMOTE_DIR} && rm -f {REMOTE_DIR}/acm-*")

    # 2. SFTP-передача бинарей
    sftp = tc.open_sftp()
    for name in BINARIES:
        local = os.path.join(DIST, name)
        remote = f"{REMOTE_DIR}/{name}"
        size = os.path.getsize(local)
        print(f"[*] scp {name} ({size} B) -> {remote}")
        sftp.put(local, remote)
        sftp.chmod(remote, 0o755)
    sftp.close()

    print()
    print("=== file inventory on module ===")
    run(tc, f"ls -la {REMOTE_DIR}")
    run(tc, f"file {REMOTE_DIR}/* 2>/dev/null || /usr/bin/file {REMOTE_DIR}/*")

    print()
    print("=== checking glibc compatibility for acm-cryptod ===")
    run(tc, f"ldd {REMOTE_DIR}/acm-cryptod 2>&1 | head -10")

    print()
    print("=== running acm-cryptod --help ===")
    run(tc, f"{REMOTE_DIR}/acm-cryptod --help 2>&1")

    print()
    print("=== running acm-cryptod (no config — expect graceful skeleton output) ===")
    run(tc, f"RUST_LOG=info {REMOTE_DIR}/acm-cryptod 2>&1 || true")

    print()
    print("=== running acm-agent ===")
    run(tc, f"{REMOTE_DIR}/acm-agent 2>&1")

    print()
    print("=== running acm-cli ===")
    run(tc, f"{REMOTE_DIR}/acm-cli 2>&1")

    tc.close()
    jc.close()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] ERROR ({type(e).__name__}): {e}")
        sys.exit(1)
