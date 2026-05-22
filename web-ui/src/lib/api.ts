// Thin fetch wrapper over the agent's REST API.
// Types mirror Go internal/ipc.StatusReport / handlers.

export interface SystemSnapshot {
  taken_at: string;
  uptime_s: number;
  load_avg: [number, number, number];
  cpu: {
    cores: number;
    busy_pct: number;
    per_core_pct: number[];
  };
  memory: {
    total_kb: number;
    available_kb: number;
    free_kb: number;
    buffers_kb: number;
    cached_kb: number;
    used_pct: number;
  };
  filesystems: Array<{
    mount: string;
    size_kb: number;
    used_kb: number;
    used_pct: number;
  }>;
  sensors: Array<{ name: string; temp_c: number }>;
  interfaces: Array<{
    name: string;
    up: boolean;
    mtu: number;
    rx_bytes: number;
    tx_bytes: number;
    rx_packets: number;
    tx_packets: number;
    rx_errors: number;
    tx_errors: number;
    rx_dropped: number;
    tx_dropped: number;
    rx_bps_now: number;
    tx_bps_now: number;
  }>;
  processes: Array<{
    name: string;
    running: boolean;
    pid: number;
    rss_kb: number;
    cpu_pct: number;
    threads: number;
    start_unix_s: number;
    uptime_s: number;
    state: string;
    exe_path: string;
    binary_size: number;
    binary_mtime_unix_s: number;
  }>;
}

export interface StatusReport {
  version: string;
  running: boolean;
  uptime_s: number;
  active_provider: string;
  active_key_id: number | null;
  packets_sealed: number;
  packets_opened: number;
  crypto_errors: number;
}

export interface ApiError {
  error: string;
  status: number;
}

async function req<T>(input: string, init?: RequestInit): Promise<T> {
  const r = await fetch(input, {
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!r.ok) {
    let msg = r.statusText;
    try {
      const j = await r.json();
      if (j.error) msg = j.error;
    } catch {}
    const err: ApiError = { error: msg, status: r.status };
    throw err;
  }
  return r.json() as Promise<T>;
}

export const api = {
  status(): Promise<StatusReport> {
    return req<StatusReport>("/api/v1/status");
  },

  system(): Promise<SystemSnapshot> {
    return req<SystemSnapshot>("/api/v1/system");
  },

  rotateKey(keyId: number, algo: number, materialHex: string): Promise<{ result: string }> {
    return req<{ result: string }>("/api/v1/keys/rotate", {
      method: "POST",
      body: JSON.stringify({
        key_id: keyId,
        algo,
        material_hex: materialHex,
      }),
    });
  },

  // Plain fetch — caller decides on text vs json.
  async healthz(): Promise<string> {
    const r = await fetch("/healthz", { cache: "no-store" });
    return r.text();
  },
};

/** Algorithm registry mirrors AlgoId in Rust. */
export const ALGORITHMS = [
  { id: 0x01, name: "AES-128-GCM", key_bytes: 16, supported: true },
  { id: 0x02, name: "AES-256-GCM", key_bytes: 32, supported: true },
  { id: 0x10, name: "O'z DSt 1105", key_bytes: 32, supported: false },
] as const;

export function algorithmById(id: number) {
  return ALGORITHMS.find((a) => a.id === id);
}
