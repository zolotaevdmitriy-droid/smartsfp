"""
Открыть web-UI агента в браузере через SSH-туннель.

Что делает:
  1. Гарантирует, что на модуле запущены cryptod (UDS /tmp/acm/cryptod.sock)
     и agent (HTTP 127.0.0.1:9100). Если не запущены — стартует их.
  2. Открывает локальный port-forward jumphost → module:9100 на 127.0.0.1:9100.
  3. Печатает «open http://localhost:9100 в браузере».
  4. По Ctrl+C: рвёт туннель, останавливает agent и cryptod (опционально).

Использование:
  python scripts/open_agent_ui.py                # запустить демоны + туннель
  python scripts/open_agent_ui.py --keep-running # не останавливать при выходе
  python scripts/open_agent_ui.py --local-port 18080  # туннель на другой порт
"""
import argparse
import select
import socket
import socketserver
import sys
import threading
import time
import webbrowser
import paramiko

JUMP_HOST, JUMP_USER, JUMP_PASS = "178.104.223.171", "root", "Cfvgbxn38"
TARGET_HOST, TARGET_PORT = "192.168.0.99", 22
TARGET_USER, TARGET_PASS = "user", "PleaseChangeTheUserPassword"

REMOTE_DIR = "/home/user/acm-uz"
SOCKET = "/tmp/acm/cryptod.sock"
AGENT_LISTEN = "127.0.0.1:9100"


def connect():
    last_err = None
    for attempt in range(1, 4):
        try:
            print(f"[*] ssh jumphost {JUMP_USER}@{JUMP_HOST} (attempt {attempt})")
            jc = paramiko.SSHClient()
            jc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
            jc.connect(JUMP_HOST, port=22, username=JUMP_USER, password=JUMP_PASS,
                       timeout=20, banner_timeout=30, auth_timeout=20,
                       allow_agent=False, look_for_keys=False)
            sock = jc.get_transport().open_channel(
                "direct-tcpip",
                (TARGET_HOST, TARGET_PORT), ("127.0.0.1", 0),
                timeout=30)
            print(f"[*] ssh module {TARGET_USER}@{TARGET_HOST}")
            tc = paramiko.SSHClient()
            tc.set_missing_host_key_policy(paramiko.AutoAddPolicy())
            tc.connect(TARGET_HOST, port=TARGET_PORT, username=TARGET_USER,
                       password=TARGET_PASS, sock=sock,
                       timeout=30, banner_timeout=30, auth_timeout=20,
                       allow_agent=False, look_for_keys=False)
            return jc, tc
        except (paramiko.SSHException, OSError) as e:
            last_err = e
            print(f"[!] attempt {attempt}: {type(e).__name__}: {e}")
            try:
                jc.close()
            except Exception:
                pass
            if attempt < 3:
                time.sleep(2)
    raise last_err


def run(tc, cmd, timeout=10):
    _, stdout, stderr = tc.exec_command(cmd, timeout=timeout)
    return (stdout.channel.recv_exit_status(),
            stdout.read().decode(errors="replace"),
            stderr.read().decode(errors="replace"))


def ensure_daemons(tc):
    """Стартует cryptod + agent если не запущены. Возвращает True если
    оба здоровы."""
    # Уже работает?
    _, out, _ = run(tc, "pgrep -a acm-cryptod; echo ---; pgrep -a acm-agent")
    print("[*] процессы на модуле:")
    for ln in out.strip().splitlines():
        print(f"      {ln}")

    cryptod_alive = "acm-cryptod" in out
    agent_alive = "acm-agent" in out

    if not cryptod_alive:
        print("[*] стартую cryptod через setsid -f")
        run(tc, f"mkdir -p /tmp/acm && setsid -f {REMOTE_DIR}/acm-cryptod "
                f"--ipc-socket {SOCKET} > /tmp/acm/cryptod.log 2>&1 < /dev/null")
        # ждём появления сокета
        for _ in range(50):
            rc, out, _ = run(tc, f"test -S {SOCKET} && echo OK")
            if "OK" in out: break
            time.sleep(0.1)
        else:
            print("[!] cryptod socket не появился")
            _, log, _ = run(tc, "cat /tmp/acm/cryptod.log")
            print(log)
            return False
        print("[+] cryptod ok")
    else:
        print("[+] cryptod уже запущен")

    if not agent_alive:
        print("[*] стартую agent через setsid -f")
        run(tc, f"setsid -f {REMOTE_DIR}/acm-agent "
                f"--cryptod-socket {SOCKET} --listen {AGENT_LISTEN} "
                f"> /tmp/acm/agent.log 2>&1 < /dev/null")
        # health check
        for _ in range(50):
            rc, _, _ = run(tc,
                f"python3 -c \"import urllib.request; urllib.request.urlopen('http://{AGENT_LISTEN}/healthz', timeout=1)\" 2>/dev/null")
            if rc == 0: break
            time.sleep(0.1)
        else:
            print("[!] agent не отвечает на /healthz")
            _, log, _ = run(tc, "cat /tmp/acm/agent.log")
            print(log)
            return False
        print("[+] agent ok")
    else:
        print("[+] agent уже запущен")

    return True


# ----------------------------------------------------------------
# SSH local port forward — копируем pattern из paramiko/demos
# ----------------------------------------------------------------
class ForwardServer(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


class ForwardHandler(socketserver.BaseRequestHandler):
    chain_host = ""
    chain_port = 0
    ssh_transport = None

    def handle(self):
        try:
            chan = self.ssh_transport.open_channel(
                "direct-tcpip",
                (self.chain_host, self.chain_port),
                self.request.getpeername(),
                timeout=10,
            )
        except Exception as e:
            print(f"[!] forward channel rejected: {e}")
            return
        if chan is None:
            return
        try:
            while True:
                r, _, _ = select.select([self.request, chan], [], [], 1)
                if self.request in r:
                    data = self.request.recv(4096)
                    if len(data) == 0: break
                    chan.send(data)
                if chan in r:
                    data = chan.recv(4096)
                    if len(data) == 0: break
                    self.request.send(data)
        except Exception:
            pass
        finally:
            chan.close()
            self.request.close()


def start_forward(local_port, remote_host, remote_port, ssh_transport):
    class Handler(ForwardHandler):
        chain_host = remote_host
        chain_port = remote_port
    Handler.ssh_transport = ssh_transport
    srv = ForwardServer(("127.0.0.1", local_port), Handler)
    t = threading.Thread(target=srv.serve_forever, daemon=True)
    t.start()
    return srv


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--local-port", type=int, default=9100,
                    help="локальный порт на вашей машине (default 9100)")
    ap.add_argument("--keep-running", action="store_true",
                    help="не останавливать cryptod/agent при выходе")
    ap.add_argument("--no-browser", action="store_true",
                    help="не открывать браузер автоматически")
    args = ap.parse_args()

    jc, tc = connect()

    if not ensure_daemons(tc):
        sys.exit(2)

    # Локальный port forward 127.0.0.1:LOCAL → module:9100
    print(f"[*] port forward 127.0.0.1:{args.local_port} -> "
          f"{TARGET_HOST}:9100 через jumphost")
    forward_srv = start_forward(
        local_port=args.local_port,
        remote_host="127.0.0.1",  # на стороне модуля
        remote_port=9100,
        ssh_transport=tc.get_transport(),
    )

    url = f"http://localhost:{args.local_port}"
    print()
    print("=" * 60)
    print(f"  ОТКРЫТЬ В БРАУЗЕРЕ:  {url}")
    print("=" * 60)
    print()
    print(f"Endpoints:")
    print(f"  {url}/                — web UI")
    print(f"  {url}/api/v1/status   — JSON-статус")
    print(f"  {url}/api/v1/keys/rotate — POST для ротации ключа")
    print(f"  {url}/metrics         — Prometheus exposition")
    print(f"  {url}/healthz         — agent alive")
    print(f"  {url}/readyz          — cryptod reachable")
    print()
    print("Ctrl+C чтобы прекратить.")

    if not args.no_browser:
        try:
            webbrowser.open(url)
        except Exception:
            pass

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        print("\n[*] Ctrl+C, останавливаю port forward")
        forward_srv.shutdown()
        forward_srv.server_close()
        if not args.keep_running:
            print("[*] останавливаю cryptod + agent на модуле")
            run(tc, "pkill -f acm-agent 2>/dev/null; "
                    "pkill -f acm-cryptod 2>/dev/null; "
                    f"rm -f {SOCKET}")
            print("[+] чисто")
        else:
            print("[+] cryptod + agent оставлены работающими (--keep-running)")
        tc.close(); jc.close()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"[!] {type(e).__name__}: {e}")
        sys.exit(1)
