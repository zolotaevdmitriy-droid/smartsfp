"""
End-to-end проверка Encrypt/Decrypt через UDS на ISM4120I.

Safety:
  - Cryptod запускается от user в /tmp/acm/cryptod.sock — никаких прав.
  - Всё в /tmp и /home/user/, никаких /etc, /usr, /opt, /var.
  - В конце всегда pkill + удаление сокета (try/finally).
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
BINARIES = ("acm-cryptod", "acm-cli", "acm-encdec-test")


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


def run(tc, cmd, timeout=60):
    stdin, stdout, stderr = tc.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    rc = stdout.channel.recv_exit_status()
    return rc, out, err


def cleanup(tc):
    """Always runs — kill cryptod, remove socket."""
    run(tc, "pkill -f acm-cryptod 2>/dev/null || true")
    run(tc, f"rm -f {SOCKET} 2>/dev/null || true")


def main():
    print("[*] connect"); jc, tc = connect(); print("[+] ready")
    try:
        # 0. SAFETY preflight: verify user shell is responsive
        rc, out, _ = run(tc, "whoami && uptime")
        print(f"    pre: {out.strip()}")
        assert "user" in out

        # 1. Push binaries
        sftp = tc.open_sftp()
        run(tc, f"mkdir -p {REMOTE_DIR}")
        for name in BINARIES:
            local = os.path.join(DIST, name)
            remote = f"{REMOTE_DIR}/{name}"
            size = os.path.getsize(local)
            print(f"[*] scp {name} ({size} B)")
            sftp.put(local, remote); sftp.chmod(remote, 0o755)
        sftp.close()

        # 2. Clean & start cryptod
        cleanup(tc)
        run(tc, "mkdir -p /tmp/acm")
        rc, out, _ = run(tc, f"nohup {REMOTE_DIR}/acm-cryptod --ipc-socket {SOCKET} "
                              f"> /tmp/acm/cryptod.log 2>&1 & echo pid=$!")
        print(f"[*] {out.strip()}")
        # Wait socket
        deadline = time.time() + 5
        ok = False
        while time.time() < deadline:
            rc, out, _ = run(tc, f"test -S {SOCKET} && echo OK")
            if "OK" in out: ok = True; break
            time.sleep(0.3)
        if not ok:
            _, log, _ = run(tc, "cat /tmp/acm/cryptod.log")
            print("[!] socket missing. cryptod log:\n", log)
            sys.exit(2)
        print(f"[+] socket: {SOCKET}")

        # 3. Provision a key (32 bytes for AES-256-GCM)
        keyhex = secrets.token_hex(32)
        print(f"[*] rotate-key 1 2 <{keyhex[:12]}...>")
        rc, out, err = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} rotate-key 1 2 {keyhex}")
        print(f"    rc={rc} {out.strip() or err.strip()}")
        assert rc == 0

        # 4. Run encdec-test
        print(f"\n[*] running acm-encdec-test")
        rc, out, err = run(tc, f"{REMOTE_DIR}/acm-encdec-test --socket {SOCKET}", timeout=60)
        print(out, end="")
        if err.strip():
            print("stderr:", err)
        assert rc == 0, f"encdec-test exited {rc}"
        assert "PASS" in out

        # 5. Read final status one more time for the journal
        print("\n[*] final status")
        rc, out, _ = run(tc, f"{REMOTE_DIR}/acm-cli --socket {SOCKET} status")
        print(out, end="")

        print("\n[+] OK — all assertions passed")
    finally:
        cleanup(tc)
        # SAFETY post-check — SSH still works
        rc, out, _ = run(tc, "echo post-cleanup-ssh-ok")
        assert "ok" in out, "post-cleanup SSH broken!"
        print("[+] post-cleanup SSH still responsive")
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
