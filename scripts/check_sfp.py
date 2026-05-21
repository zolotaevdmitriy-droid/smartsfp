"""
Подключение к Smart SFP ISM4120I 192.168.0.99 через jumphost root@178.104.223.171.
Обе авторизации — по паролю. Целевая модель — ISM4120I.
"""
import sys
import paramiko

JUMP_HOST = "178.104.223.171"
JUMP_USER = "root"
JUMP_PASS = "Cfvgbxn38"

TARGET_HOST = "192.168.0.99"
TARGET_PORT = 22
TARGET_USER = "user"
TARGET_PASS = "PleaseChangeTheUserPassword"

IDENT_CMDS = [
    ("issue", "cat /etc/issue 2>/dev/null"),
    ("hostname", "cat /etc/hostname 2>/dev/null"),
    ("uname", "uname -a"),
    ("os-release", "cat /etc/os-release 2>/dev/null"),
    ("cpuinfo", "cat /proc/cpuinfo | head -20"),
    ("memory", "free -m"),
    ("disk", "df -h"),
    ("net", "ip -br a 2>/dev/null"),
    ("kernel-mods", "lsmod | head -20"),
    ("dpdk", "ls /usr/include/dpdk 2>/dev/null | head; dpkg -l | grep -i dpdk 2>/dev/null"),
    ("docker", "which docker; docker --version 2>/dev/null"),
    ("uptime", "uptime"),
    ("sw-version", "cat /etc/sw-version 2>/dev/null; ls /etc/*version* 2>/dev/null"),
]


def main():
    # 1) Подключение к jumphost
    print(f"[*] Connecting to jumphost {JUMP_USER}@{JUMP_HOST} ...")
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(
        JUMP_HOST,
        port=22,
        username=JUMP_USER,
        password=JUMP_PASS,
        timeout=20,
        banner_timeout=20,
        auth_timeout=20,
        allow_agent=False,
        look_for_keys=False,
    )
    print("[+] Jumphost connected")

    # 2) direct-tcpip канал к таргету через jump
    jt = jc.get_transport()
    print(f"[*] Opening channel jump -> {TARGET_HOST}:{TARGET_PORT} ...")
    sock = jt.open_channel(
        kind="direct-tcpip",
        dest_addr=(TARGET_HOST, TARGET_PORT),
        src_addr=("127.0.0.1", 0),
        timeout=20,
    )
    print("[+] Channel open")

    # 3) Поверх канала — SSH-сессия к таргету
    print(f"[*] Authenticating to {TARGET_HOST} as {TARGET_USER}/<password from docs>...")
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(
        hostname=TARGET_HOST,
        port=TARGET_PORT,
        username=TARGET_USER,
        password=TARGET_PASS,
        sock=sock,
        timeout=20,
        banner_timeout=20,
        auth_timeout=20,
        allow_agent=False,
        look_for_keys=False,
    )
    print(f"[+] AUTH OK: {TARGET_USER}@{TARGET_HOST}")
    print()

    # 4) Идентификация
    print("=== Module identification ===")
    for tag, cmd in IDENT_CMDS:
        stdin, stdout, stderr = tc.exec_command(cmd, timeout=10)
        out = stdout.read().decode(errors="replace").rstrip()
        err = stderr.read().decode(errors="replace").rstrip()
        print(f"--- [{tag}] $ {cmd}")
        if out:
            print(out)
        if err and not out:
            print(f"(stderr) {err}")
        print()

    tc.close()
    jc.close()
    print("[*] Done.")


if __name__ == "__main__":
    try:
        main()
    except paramiko.AuthenticationException as e:
        print(f"[!] AUTH FAILED: {e}")
        sys.exit(2)
    except Exception as e:
        print(f"[!] ERROR ({type(e).__name__}): {e}")
        sys.exit(1)
