"""
Read-only разведка крипто-инструментов на ISM4120I.
Что есть под user, что требует root, что вообще не установлено.
"""
import sys
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

CMDS = [
    # 1. OpenSSL CLI
    ("openssl.path",       "command -v openssl; ls /usr/bin/openssl /usr/local/bin/openssl 2>/dev/null"),
    ("openssl.ver",        "openssl version -a 2>&1"),
    ("openssl.algos.list", "openssl list -cipher-algorithms 2>&1 | head -30"),

    # 2. MUSDK SAM samples
    ("musdk.bin",          "ls -la /usr/bin/musdk_* 2>/dev/null"),
    ("musdk.sam-kat.help", "musdk_sam_kat --help 2>&1 | head -30 || musdk_sam_kat -h 2>&1 | head -30"),
    ("musdk.sam-single.help","musdk_sam_single --help 2>&1 | head -30 || musdk_sam_single -h 2>&1 | head -30"),

    # 3. Cipher devs (DPDK / kernel)
    ("crypto.kmod",        "lsmod | grep -iE 'crypt|cesa|safexcel|inside|mvsam|sam|chach|sha|aes' "),
    ("crypto.sysfs",       "ls /sys/class/crypto 2>/dev/null | head -20; ls /sys/kernel/cryptouser 2>/dev/null"),
    ("crypto.devs",        "ls /dev/uio* /dev/crypto* /dev/cryptodev* 2>/dev/null"),

    # 4. cpuinfo crypto features
    ("cpu.features",       "grep -m1 Features /proc/cpuinfo"),

    # 5. iperf / siege / wrk для bench
    ("net.tools",          "command -v iperf3 nuttcp pktgen ethtool tcpdump"),

    # 6. python crypto
    ("py.crypto",          "python3 -c 'import cryptography; print(cryptography.__version__)' 2>&1; python3 -c 'from cryptography.hazmat.primitives.ciphers.aead import AESGCM; print(\"AESGCM ok\")' 2>&1"),

    # 7. sudo / root доступ
    ("sudo",               "sudo -n -l 2>&1 | head -5; id"),

    # 8. /proc/crypto — kernel crypto API
    ("proc.crypto",        "cat /proc/crypto 2>/dev/null | head -60"),

    # 9. cryptodev kernel module
    ("cryptodev",          "modinfo cryptodev 2>&1 | head -3"),
]

def main():
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
    print(f"[+] {TARGET_USER}@{TARGET_HOST}")

    for tag, cmd in CMDS:
        stdin, stdout, stderr = tc.exec_command(cmd, timeout=20)
        out = stdout.read().decode(errors="replace").rstrip()
        err = stderr.read().decode(errors="replace").rstrip()
        print(f"\n--- [{tag}] ---")
        print(f"$ {cmd}")
        if out:
            print(out)
        if err and not out:
            print(f"(stderr) {err}")
        elif not out and not err:
            print("(empty)")

    tc.close(); jc.close()

if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}"); sys.exit(1)
