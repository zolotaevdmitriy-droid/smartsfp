"""
E2E проверка agent (prometheus+REST+UI) на ISM4120I.

Архитектура: заливаем все бинари + один bash-скрипт, который сам стартует
cryptod+agent, прогоняет проверки и чистит за собой. paramiko дёргает его
одним заходом — никаких хитростей с backgrounding'ом через SSH.

Safety:
  - всё от user (не root), на 127.0.0.1, в /tmp и /home/user;
  - скрипт сам пишет EXIT-trap для cleanup;
  - post-SSH check после.
"""
import os, sys
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

DIST = os.path.join(os.path.dirname(__file__), "..", "dist")
SCRIPT = os.path.join(os.path.dirname(__file__), "agent-test-on-module.sh")
REMOTE_DIR = "/home/user/acm-uz"
REMOTE_SCRIPT = "/tmp/acm/agent-test.sh"
BIN = ("acm-cryptod", "acm-agent", "acm-cli", "acm-encdec-test")


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


def main():
    print("[*] connect"); jc, tc = connect(); print("[+] ready")

    # Preflight
    _, stdout, _ = tc.exec_command("whoami && uptime", timeout=10)
    print(f"    pre: {stdout.read().decode().strip()}")

    sftp = tc.open_sftp()
    tc.exec_command(f"mkdir -p {REMOTE_DIR} /tmp/acm").channel.recv_exit_status() \
        if False else None  # paramiko quirk handled below
    _, so, _ = tc.exec_command(f"mkdir -p {REMOTE_DIR} /tmp/acm", timeout=10)
    so.read()

    for name in BIN:
        local = os.path.join(DIST, name)
        sz = os.path.getsize(local)
        print(f"[*] scp {name} ({sz} B)")
        sftp.put(local, f"{REMOTE_DIR}/{name}")
        sftp.chmod(f"{REMOTE_DIR}/{name}", 0o755)
    print(f"[*] scp agent-test.sh")
    sftp.put(SCRIPT, REMOTE_SCRIPT)
    sftp.chmod(REMOTE_SCRIPT, 0o755)
    sftp.close()

    # Run the test script — one exec_command, one return code.
    print(f"\n[*] running {REMOTE_SCRIPT}")
    print("-" * 60)
    _, stdout, stderr = tc.exec_command(f"bash {REMOTE_SCRIPT}", timeout=120)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    print(out, end="")
    if err.strip():
        print("\n--- stderr ---")
        print(err)
    print("-" * 60)
    print(f"script exit code: {rc}")

    # Post-cleanup sanity (script does cleanup in trap, we just double-check)
    _, stdout, _ = tc.exec_command(
        "pkill -f acm-agent 2>/dev/null; pkill -f acm-cryptod 2>/dev/null; "
        "rm -f /tmp/acm/cryptod.sock; echo post-cleanup-ok",
        timeout=10)
    last = stdout.read().decode().strip()
    print(f"[+] {last}")

    tc.close(); jc.close()

    if rc != 0 or "ALL_TESTS_PASSED" not in out:
        sys.exit(2)
    print("\n[OK] all assertions passed")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"\n[!] {type(e).__name__}: {e}")
        sys.exit(1)
