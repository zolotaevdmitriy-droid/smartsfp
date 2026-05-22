#!/bin/bash
# Runs entirely ON THE MODULE. Started/cleaned up by paramiko in one shot.
# Uses wget (curl is not installed in stock Debian minimal on ISM4120I).

set -u
LANG=C.UTF-8

BIN_DIR=/home/user/acm-uz
SOCK=/tmp/acm/cryptod.sock
LISTEN=127.0.0.1:9100
AGENT_URL="http://${LISTEN}"

cleanup() {
  pkill -f "${BIN_DIR}/acm-agent"     2>/dev/null || true
  pkill -f "${BIN_DIR}/acm-cryptod"   2>/dev/null || true
  rm -f "${SOCK}" 2>/dev/null
}
trap cleanup EXIT INT TERM

assert() {
  if eval "$2"; then
    echo "PASS: $1"
  else
    echo "FAIL: $1  (cond: $2)"
    exit 11
  fi
}

# HTTP helpers via python3 — module has no curl/wget but python3 is installed.
http_get_check() {  # only check 200, body discarded
  python3 -c "
import sys, urllib.request
try:
    urllib.request.urlopen('$1', timeout=2).read()
except Exception as e:
    sys.exit(1)
"
}
http_get() {       # body to stdout
  python3 -c "
import urllib.request
print(urllib.request.urlopen('$1', timeout=5).read().decode(), end='')
"
}
http_post_json() { # url, json_body
  # Use urllib but catch HTTPError so 4xx returns the error body instead
  # of a stacktrace — we want to assert on response content either way.
  python3 -c "
import sys, urllib.request, urllib.error
req = urllib.request.Request('$1',
    data=b'''$2''',
    headers={'Content-Type': 'application/json'})
try:
    print(urllib.request.urlopen(req, timeout=5).read().decode(), end='')
except urllib.error.HTTPError as e:
    sys.stdout.write(e.read().decode())
"
}

contains() {
  grep -F -q -- "$2" <<<"$1"
}

mkdir -p /tmp/acm

# Pre-cleanup any leftovers from a previous failed run.
cleanup
sleep 0.3

echo "=== start cryptod ==="
setsid -f "${BIN_DIR}/acm-cryptod" --ipc-socket "${SOCK}" \
    > /tmp/acm/cryptod.log 2>&1 < /dev/null
for i in $(seq 1 50); do
  [[ -S "${SOCK}" ]] && break
  sleep 0.1
done
[[ -S "${SOCK}" ]] || { echo "FAIL: cryptod socket missing"; cat /tmp/acm/cryptod.log; exit 12; }
echo "cryptod ok"

echo "=== start agent ==="
setsid -f "${BIN_DIR}/acm-agent" \
    --cryptod-socket "${SOCK}" --listen "${LISTEN}" \
    > /tmp/acm/agent.log 2>&1 < /dev/null
for i in $(seq 1 50); do
  http_get_check "${AGENT_URL}/healthz" && break
  sleep 0.1
done
http_get_check "${AGENT_URL}/healthz" || {
  echo "FAIL: agent /healthz never responded"
  cat /tmp/acm/agent.log
  exit 13
}
echo "agent ok"

echo "=== probes ==="
healthz=$(http_get "${AGENT_URL}/healthz")
readyz=$(http_get  "${AGENT_URL}/readyz")
assert "/healthz returns ok"    "[[ \$(printf %s '$healthz') == ok* ]]"
assert "/readyz  returns ready" "[[ \$(printf %s '$readyz')  == ready* ]]"

echo "=== status (before rotate) ==="
status1=$(http_get "${AGENT_URL}/api/v1/status")
echo "$status1"
assert "status has version"     "contains '$status1' 'version'"
assert "active_key_id is null"  "contains '$status1' '\"active_key_id\":null'"

echo "=== rotate-key via REST ==="
KEYHEX=$(python3 -c "import secrets; print(secrets.token_hex(32))")
rotate=$(http_post_json "${AGENT_URL}/api/v1/keys/rotate" \
    "{\"key_id\":42,\"algo\":2,\"material_hex\":\"${KEYHEX}\"}")
echo "$rotate"
assert "rotate returns ok"  "contains '$rotate' '\"result\":\"ok\"'"

echo "=== status (after rotate) ==="
status2=$(http_get "${AGENT_URL}/api/v1/status")
echo "$status2"
assert "active_key_id == 42" "contains '$status2' '\"active_key_id\":42'"

echo "=== /metrics ==="
metrics=$(http_get "${AGENT_URL}/metrics")
echo "$metrics" | grep -E '^acm_cryptod_' || true
assert "acm_cryptod_up == 1"            "contains '$metrics' 'acm_cryptod_up 1'"
assert "version_info present"            "contains '$metrics' 'acm_cryptod_version_info{'"
assert "active_key_id metric == 42"      "contains '$metrics' 'acm_cryptod_active_key_id 42'"
assert "sealed counter == 0 pre-encdec"  "contains '$metrics' 'acm_cryptod_packets_sealed_total 0'"

echo "=== / (web UI) ==="
http_get "${AGENT_URL}/" > /tmp/acm/ui.html
ui_size=$(wc -c < /tmp/acm/ui.html)
echo "UI page: ${ui_size} bytes"
# Use direct grep on the file — eval-based assert doesn't survive HTML quoting.
if grep -q '<title>ACM-UZ Agent</title>' /tmp/acm/ui.html; then
    echo "PASS: / has <title>"
else
    echo "FAIL: / missing <title>"; exit 11
fi
if grep -q 'cryptod status' /tmp/acm/ui.html; then
    echo "PASS: / has 'cryptod status'"
else
    echo "FAIL: / missing 'cryptod status'"; exit 11
fi
if grep -q 'rotate-form' /tmp/acm/ui.html; then
    echo "PASS: / has rotate-key form"
else
    echo "FAIL: / missing rotate form"; exit 11
fi

echo "=== encdec-test ==="
"${BIN_DIR}/acm-encdec-test" --socket "${SOCK}" > /tmp/acm/encdec.out 2>&1
encdec_rc=$?
echo "encdec-test rc=${encdec_rc}"
sed 's/^/    /' /tmp/acm/encdec.out | head -20
assert "encdec-test PASSed" "[[ ${encdec_rc} -eq 0 ]]"

echo "=== /metrics (after encdec) ==="
metrics=$(http_get "${AGENT_URL}/metrics")
echo "$metrics" | grep -E '^acm_cryptod_(packets|errors)' || true
assert "sealed == 10" "contains '$metrics' 'acm_cryptod_packets_sealed_total 10'"
assert "opened == 9"  "contains '$metrics' 'acm_cryptod_packets_opened_total 9'"
assert "errors == 1"  "contains '$metrics' 'acm_cryptod_errors_total 1'"

echo "=== agent access log tail ==="
tail -15 /tmp/acm/agent.log | sed 's/^/    /'

echo "ALL_TESTS_PASSED"
exit 0
