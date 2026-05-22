"""
Offline-установка openssl на ISM4120I.

Модуль без интернета. Скачиваем .deb на jumphost (у него интернет),
scp'ом отдаём в /tmp/ модуля, ставим через `dpkg -i`.

Дополнительно (если нужно) докатываем зависимости.
"""
import os
import re
import sys
import time
import shlex
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"
ROOT_PASS = "PleaseChangeTheRootPassword"

# Debian bookworm ARM64 openssl + минимальные зависимости.
# Берём из main debian mirror.
DEB_PKGS = [
    "http://ftp.debian.org/debian/pool/main/o/openssl/openssl_3.0.18-1~deb12u2_arm64.deb",
]

TMP_REMOTE = "/tmp/openssl-offline"


def jump_ssh():
    jc = paramiko.SSHClient()
    jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
               timeout=20, allow_agent=False, look_for_keys=False)
    return jc


def target_ssh(jc):
    sock = jc.get_transport().open_channel(
        "direct-tcpip", (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0), timeout=20)
    tc = paramiko.SSHClient()
    tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER,
               password=TARGET_PASS, sock=sock, timeout=20,
               allow_agent=False, look_for_keys=False)
    return tc


def run(ssh, cmd, timeout=180):
    stdin, stdout, stderr = ssh.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode(errors="replace")
    err = stderr.read().decode(errors="replace")
    return stdout.channel.recv_exit_status(), out, err


def run_root(tc, cmd, timeout=180):
    su = f"su -c {shlex.quote(cmd)}"
    chan = tc.get_transport().open_session()
    chan.settimeout(timeout + 5)
    chan.get_pty(term="dumb", width=200, height=50)
    chan.exec_command(su)
    buf = bytearray()
    sent_pw = False
    start = time.time()
    while True:
        if chan.recv_ready():
            buf.extend(chan.recv(16384))
        if not sent_pw and b"assword:" in buf:
            chan.send(ROOT_PASS + "\n"); sent_pw = True
        if chan.exit_status_ready() and not chan.recv_ready():
            time.sleep(0.05)
            while chan.recv_ready():
                buf.extend(chan.recv(16384))
            break
        if time.time() - start > timeout:
            chan.close(); raise TimeoutError(cmd)
        time.sleep(0.03)
    rc = chan.recv_exit_status()
    text = buf.decode(errors="replace")
    text = re.sub(r"(?s)^.*?[Pp]assword:[^\r\n]*\r?\n", "", text, count=1)
    return rc, text


def main():
    print(f"[*] connect jumphost…")
    jc = jump_ssh()
    print(f"[+] jumphost {JUMP_USER}@{JUMP_HOST}")
    print(f"[*] connect module…")
    tc = target_ssh(jc)
    print(f"[+] module {TARGET_USER}@{TARGET_HOST}")

    # 1. Скачиваем .deb на jumphost в /tmp
    run(jc, "mkdir -p /tmp/acm-debs && cd /tmp/acm-debs && rm -f *.deb")
    # Сначала найдём актуальное имя файла (.deb версия меняется).
    print("[*] discover current openssl_arm64.deb on debian.org…")
    # bookworm-specific: only packages with ~deb12 (=рассчитан под bookworm).
    DEB12_RE = "openssl_[0-9][^\"]*~deb12u[0-9]+_arm64\\.deb"
    rc, out, err = run(jc,
        f"wget -qO - http://ftp.debian.org/debian/pool/main/o/openssl/ "
        f"| grep -oE '{DEB12_RE}' | sort -uV | tail -1",
        timeout=60)
    candidate = out.strip().splitlines()[-1] if out.strip() else ""
    base_url = "http://ftp.debian.org/debian/pool/main/o/openssl/"
    if not candidate:
        print(f"    not in main pool, trying security pool")
        rc, out, err = run(jc,
            f"wget -qO - http://security.debian.org/debian-security/pool/updates/main/o/openssl/ "
            f"| grep -oE '{DEB12_RE}' | sort -uV | tail -1",
            timeout=60)
        candidate = out.strip().splitlines()[-1] if out.strip() else ""
        base_url = "http://security.debian.org/debian-security/pool/updates/main/o/openssl/"
    if not candidate:
        print("    cannot find any openssl_arm64.deb"); sys.exit(2)
    url = base_url + candidate
    print(f"    -> {url}")

    print(f"[*] wget {candidate} on jumphost…")
    rc, out, err = run(jc, f"cd /tmp/acm-debs && wget -q '{url}' && ls -la {candidate}", timeout=120)
    if rc != 0:
        print(f"    rc={rc} stderr={err.strip()}"); sys.exit(2)
    print(f"    {out.strip()}")
    fnames = [candidate]

    # 2. Подготовка каталога на модуле
    print(f"[*] mkdir {TMP_REMOTE} on module")
    run(tc, f"mkdir -p {TMP_REMOTE}")

    # 3. Передача файлов с jumphost на модуль (jumphost -> module через локальный pipe)
    # Идём через SFTP: открываем sftp с модулем, читаем файл из jumphost локально и записываем.
    sftp_target = tc.open_sftp()
    sftp_jump = jc.open_sftp()
    for fname in fnames:
        jump_path = f"/tmp/acm-debs/{fname}"
        remote_path = f"{TMP_REMOTE}/{fname}"
        print(f"[*] transfer {fname}  (jumphost -> module)")
        with sftp_jump.open(jump_path, "rb") as src, \
             sftp_target.open(remote_path, "wb") as dst:
            while True:
                chunk = src.read(64 * 1024)
                if not chunk: break
                dst.write(chunk)
        stat = sftp_target.stat(remote_path)
        print(f"    {remote_path}: {stat.st_size} bytes")
    sftp_target.close()
    sftp_jump.close()

    # 4. dpkg -i как root. Корень модуля смонтирован ro — remount + revert.
    print(f"[*] mount -o rw,remount / (root)")
    rc, out = run_root(tc, "mount -o rw,remount / && mount | grep ' on / '", timeout=30)
    print(f"    rc={rc} :: {out.strip()}")
    if rc != 0:
        print("[!] cannot remount rw")
        sys.exit(2)

    print(f"[*] dpkg -i на модуле (root)…")
    rc, out = run_root(tc,
        f"PATH=/usr/local/sbin:/usr/sbin:/sbin:/usr/local/bin:/usr/bin:/bin "
        f"dpkg -i {TMP_REMOTE}/*.deb 2>&1",
        timeout=180)

    # Возвращаем ro в любом случае.
    print(f"[*] mount -o ro,remount / (root)")
    rc2, out2 = run_root(tc, "mount -o ro,remount / && mount | grep ' on / '", timeout=30)
    print(f"    rc={rc2} :: {out2.strip()}")
    print(f"    rc={rc}")
    for ln in out.rstrip().splitlines()[:30]:
        print(f"    | {ln}")

    # 5. Проверка
    print(f"[*] verify openssl")
    rc, out, err = run(tc, "openssl version -a 2>&1")
    print(f"    rc={rc}")
    for ln in out.rstrip().splitlines():
        print(f"    | {ln}")

    tc.close(); jc.close()
    print("\n[+] openssl installed" if rc == 0 else "\n[!] openssl install FAILED")
    sys.exit(0 if rc == 0 else 1)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}"); sys.exit(1)
