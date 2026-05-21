"""
Read-only разведка ISM4120I 192.168.0.99 через jumphost.
Ничего не меняем — только смотрим.
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

RECON = [
    # ---- 1. Менеджеры пакетов и их состояние ----
    ("pkg.dpkg",      "dpkg -l 2>/dev/null | wc -l ; echo '---' ; dpkg -l 2>/dev/null | head -5"),
    ("pkg.dpkg-net",  "dpkg -l 2>/dev/null | grep -iE 'dpdk|musdk|mvpp|openssl|libcrypt' | head -20"),
    ("pkg.opkg",      "command -v opkg ; opkg list-installed 2>/dev/null | head -5"),
    ("pkg.ipkg",      "command -v ipkg"),
    ("pkg.snap",      "command -v snap"),
    ("pkg.flatpak",   "command -v flatpak"),
    ("pkg.pacman",    "command -v pacman"),
    ("pkg.conda",     "command -v conda"),
    ("pkg.brew",      "command -v brew"),
    ("pkg.guix",      "command -v guix"),
    ("pkg.nix",       "command -v nix"),
    ("pkg.pip",       "command -v pip ; command -v pip3"),
    ("pkg.npm",       "command -v npm ; command -v yarn"),
    ("pkg.cargo",     "command -v cargo"),
    ("pkg.go",        "command -v go ; go version 2>/dev/null"),
    ("apt-sources",   "cat /etc/apt/sources.list 2>/dev/null ; ls /etc/apt/sources.list.d/ 2>/dev/null"),

    # ---- 2. DPDK / MUSDK / Marvell ----
    ("dpdk.headers",  "ls /usr/include/dpdk 2>/dev/null | head -10 ; ls /usr/local/include/dpdk 2>/dev/null | head -10"),
    ("dpdk.lib",      "ldconfig -p 2>/dev/null | grep -i dpdk | head -20"),
    ("dpdk.pkgconfig","pkg-config --list-all 2>/dev/null | grep -iE 'dpdk|musdk' ; pkg-config --modversion libdpdk 2>/dev/null"),
    ("dpdk.bin",      "ls -la /usr/bin/dpdk* 2>/dev/null ; ls -la /usr/local/bin/dpdk* 2>/dev/null"),
    ("dpdk.find",     "find / -xdev -type f \\( -name 'librte*' -o -name 'libdpdk*' -o -name '*.pmd' -o -name 'dpdk-*' \\) 2>/dev/null | head -30"),
    ("musdk.find",    "find / -xdev \\( -name 'libmusdk*' -o -name 'musdk*' -o -name 'mvpp*' \\) 2>/dev/null | head -30"),
    ("opt-dir",       "ls -la /opt 2>/dev/null"),
    ("usrlocal",      "ls -la /usr/local 2>/dev/null ; ls -la /usr/local/lib 2>/dev/null | head -30"),

    # ---- 3. Сетевые драйверы и hugepages ----
    ("eth.gbe0",      "ethtool -i gbe0 2>/dev/null"),
    ("eth.gbe1",      "ethtool -i gbe1 2>/dev/null"),
    ("eth.feat0",     "ethtool -k gbe0 2>/dev/null | head -30"),
    ("hugepages",     "cat /proc/meminfo | grep -i huge ; echo '---' ; ls /sys/kernel/mm/hugepages/ 2>/dev/null"),
    ("vfio",          "ls /dev/vfio 2>/dev/null ; ls /sys/bus/pci/drivers 2>/dev/null"),
    ("uio",           "ls /dev/uio* 2>/dev/null ; cat /proc/devices | grep uio"),
    ("pci",           "lspci 2>/dev/null | head -30"),
    ("interfaces",    "ip -d link show 2>/dev/null | head -60"),
    ("bridge",        "bridge link 2>/dev/null ; brctl show 2>/dev/null"),
    ("routes",        "ip route 2>/dev/null ; ip -6 route 2>/dev/null | head -10"),

    # ---- 4. Процессы и сервисы ----
    ("processes",     "ps auxf 2>/dev/null | head -60"),
    ("systemd-units", "systemctl list-units --type=service --no-pager 2>/dev/null | head -60"),
    ("systemd-fail",  "systemctl list-units --failed --no-pager 2>/dev/null"),
    ("ports",         "ss -tlnp 2>/dev/null ; echo '---udp---' ; ss -ulnp 2>/dev/null"),

    # ---- 5. Крипто и связанные библиотеки ----
    ("openssl",       "openssl version -a 2>/dev/null"),
    ("ssh-version",   "sshd -V 2>&1 | head -5 ; ssh -V 2>&1"),
    ("libs",          "ldconfig -p 2>/dev/null | grep -iE 'libssl|libcrypto|libsodium|libgcrypt|libssh' | head -20"),
    ("kmod.crypto",   "lsmod | grep -iE 'crypt|cesa|safexcel|inside|arm_crypto' "),

    # ---- 6. Vendor-специфичные артефакты ----
    ("vendor.dirs",   "ls -la /var/lib/probe 2>/dev/null ; ls -la /etc/config 2>/dev/null"),
    ("vendor.bin",    "ls -la /usr/sbin/ 2>/dev/null | grep -iE 'probe|sfp|smart|module' | head -20"),
    ("vendor.tools",  "command -v sfp-eeprom-parser ; command -v config-tool ; command -v run-klish ; command -v promisctl"),
    ("etc-list",      "ls /etc 2>/dev/null"),
    ("home-list",     "ls -la /home /root 2>/dev/null"),

    # ---- 7. Аппаратные сенсоры / DDMI / EEPROM ----
    ("sensors",       "ls /sys/class/hwmon/ 2>/dev/null ; for d in /sys/class/hwmon/hwmon*; do echo \"-- $d --\"; cat $d/name 2>/dev/null; cat $d/temp1_input 2>/dev/null; done"),
    ("eeprom",        "ls /sys/bus/i2c/devices/ 2>/dev/null ; find /sys -name eeprom 2>/dev/null | head -5"),
    ("ddmi",          "ls /var/lib/probe/ddmi 2>/dev/null"),

    # ---- 8. cgroups, capabilities, security ----
    ("cgroups",       "ls /sys/fs/cgroup 2>/dev/null | head -20 ; cat /proc/self/cgroup"),
    ("apparmor",      "command -v aa-status ; aa-status 2>/dev/null | head -10"),
    ("selinux",       "getenforce 2>/dev/null ; ls /sys/fs/selinux 2>/dev/null"),

    # ---- 9. Загрузчик и партиции eMMC ----
    ("mmc",           "ls -la /dev/mmcblk* 2>/dev/null ; fdisk -l /dev/mmcblk0 2>/dev/null | head -25"),
    ("mounts",        "mount | grep -v tmpfs"),
    ("kernel-cmdline","cat /proc/cmdline"),
    ("uboot-env",     "command -v fw_printenv ; fw_printenv 2>/dev/null | head -30"),

    # ---- 10. Конфиг сети, time, DNS ----
    ("netconf",       "cat /etc/network/interfaces 2>/dev/null ; echo '---' ; ls /etc/network/interfaces.d/ 2>/dev/null ; echo '---' ; for f in /etc/network/interfaces.d/*; do echo \"# $f\"; cat $f 2>/dev/null; done"),
    ("time",          "timedatectl 2>/dev/null ; cat /etc/timezone 2>/dev/null ; date"),
    ("ntp",           "command -v ntpq ; ntpq -p 2>/dev/null ; cat /etc/ntp.conf 2>/dev/null | head -20"),
    ("dns",           "cat /etc/resolv.conf 2>/dev/null"),

    # ---- 11. Поиск интересных файлов / артефактов ----
    ("big-files",     "find / -xdev -type f -size +5M 2>/dev/null | head -30"),
    ("recent",        "find /home /root /opt /usr/local -xdev -type f -mtime -180 2>/dev/null | head -30"),
    ("rc-local",      "cat /etc/rc.local 2>/dev/null"),
    ("modprobe",      "ls /etc/modules-load.d/ 2>/dev/null ; cat /etc/modules 2>/dev/null"),
]


def main():
    print(f"[*] Jumphost {JUMP_USER}@{JUMP_HOST} ...")
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
               timeout=20, banner_timeout=20, auth_timeout=20,
               allow_agent=False, look_for_keys=False)
    print("[+] Jumphost connected")

    jt = jc.get_transport()
    sock = jt.open_channel("direct-tcpip", (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0), timeout=20)

    print(f"[*] {TARGET_USER}@{TARGET_HOST} ...")
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER, password=TARGET_PASS,
               sock=sock, timeout=20, banner_timeout=20, auth_timeout=20,
               allow_agent=False, look_for_keys=False)
    print(f"[+] AUTH OK: {TARGET_USER}@{TARGET_HOST}\n")

    print("=" * 80)
    print("ISM4120I read-only recon")
    print("=" * 80)
    for tag, cmd in RECON:
        stdin, stdout, stderr = tc.exec_command(cmd, timeout=30)
        out = stdout.read().decode(errors="replace").rstrip()
        err = stderr.read().decode(errors="replace").rstrip()
        print(f"\n----- [{tag}] -----")
        print(f"$ {cmd}")
        if out:
            print(out)
        if err and not out:
            print(f"(stderr) {err}")
        elif not out and not err:
            print("(empty)")

    tc.close()
    jc.close()
    print("\n[*] Done.")


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] ERROR ({type(e).__name__}): {e}")
        sys.exit(1)
