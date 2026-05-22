"""
Залить свежий acm-agent на модуль и перезапустить.
Cryptod не трогаем — если работает, остаётся.

Use case: после изменений UI/handlers только в agent — быстрая итерация.
"""
import os, time, paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"
REMOTE_DIR = "/home/user/acm-uz"
SOCKET = "/tmp/acm/cryptod.sock"
LISTEN = "127.0.0.1:9100"


def main():
    print("[*] connect")
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
               timeout=20, banner_timeout=30, allow_agent=False, look_for_keys=False)
    sock = jc.get_transport().open_channel("direct-tcpip",
        (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0), timeout=20)
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER,
               password=TARGET_PASS, sock=sock, timeout=20, banner_timeout=30,
               allow_agent=False, look_for_keys=False)
    print("[+] ssh ok")

    def run(cmd, t=10):
        _, o, e = tc.exec_command(cmd, timeout=t)
        return o.channel.recv_exit_status(), o.read().decode(), e.read().decode()

    # Убить agent, оставить cryptod
    print("[*] killing acm-agent")
    run("pkill -f acm-agent 2>/dev/null || true; sleep 0.5")

    # Cryptod если не запущен — стартануть
    rc, out, _ = run("pgrep -a acm-cryptod || echo none")
    if "none" in out:
        print("[*] starting cryptod")
        run(f"mkdir -p /tmp/acm && setsid -f {REMOTE_DIR}/acm-cryptod "
            f"--ipc-socket {SOCKET} > /tmp/acm/cryptod.log 2>&1 < /dev/null")
        time.sleep(0.5)
    else:
        print(f"[+] cryptod alive: {out.strip()}")

    # Залить свежий agent
    print("[*] scp acm-agent")
    sftp = tc.open_sftp()
    local = "D:/SMART SFP/dist/acm-agent"
    sftp.put(local, f"{REMOTE_DIR}/acm-agent")
    sftp.chmod(f"{REMOTE_DIR}/acm-agent", 0o755)
    sftp.close()
    print(f"    {os.path.getsize(local)} B")

    # Стартуем agent
    print("[*] starting agent")
    run(f"setsid -f {REMOTE_DIR}/acm-agent "
        f"--cryptod-socket {SOCKET} --listen {LISTEN} "
        f"> /tmp/acm/agent.log 2>&1 < /dev/null")
    time.sleep(1)
    rc, out, _ = run(
        "python3 -c \"import urllib.request; "
        "print(urllib.request.urlopen('http://127.0.0.1:9100/healthz', timeout=2).read().decode())\"")
    if "ok" not in out:
        print("[!] agent not responding")
        _, log, _ = run("tail -30 /tmp/acm/agent.log")
        print(log)
        return
    print(f"[+] agent responding: {out.strip()}")

    # Show what's bound
    rc, out, _ = run("ps -ef | grep -E acm-cryptod\\|acm-agent | grep -v grep")
    print("[+] processes:")
    for ln in out.splitlines():
        print(f"    {ln}")

    tc.close(); jc.close()
    print("\n[+] Open http://localhost:9100 in your browser")
    print("    (через уже работающий туннель open_agent_ui.py)")


if __name__ == "__main__":
    main()
