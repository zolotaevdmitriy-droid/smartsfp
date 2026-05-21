# smartsfp — ACM-UZ

Production-grade traffic encryptor in a 1G SFP form factor. Bump-in-the-wire
TCP/IP encryption based on Uzbek national cryptography (O'z DSt 1105:2009),
deployed on the **Smart SFP ISM4120I** platform.

## Target hardware

| Parameter | Value |
|---|---|
| Device | Smart SFP ISM4120I |
| SoC | Marvell Armada 3720 (ARMv8) |
| CPU | 2 × Cortex-A53 @ 1.2 GHz |
| RAM | 1 GB |
| Storage | 4 GB eMMC |
| OS | Debian 12 (bookworm), kernel 6.1 |
| Data-plane | DPDK 25.0 + MUSDK |
| HW crypto | Marvell SAM (EIP-97) — AES, SHA, 3DES, ChaCha20 in hardware |
| Link | 1000BASE-LX, 1310 nm, LC, SMF, 20 km |

Confirmed by live recon, see `docs/recon-2026-05-21.md` (to be added).

## Architecture at a glance

```
        ┌───────────────────────────── Smart SFP ISM4120I ────────────────────────────┐
                                                                                       
                       ┌────────────────────────────────────────────────────┐          
        gbe0  ◀──────▶ │ acm-cryptod  (Rust, DPDK + MUSDK SAM)             │ ◀──────▶ gbe1
       (line)          │   - AES-256-GCM via HW (Phase 1)                  │      (host)
                       │   - O'z DSt 1105 in software (Phase 2)            │
                       │   - O'z DSt 1105 certified (Phase 3)              │
                       └─────────────────────────┬──────────────────────────┘          
                                                 │  UDS + shared-memory ring           
                                                 ▼                                     
                       ┌────────────────────────────────────────────────────┐          
                       │ acm-agent  (Go)                                    │          
                       │   - Local web UI (Svelte, embedded)                │          
                       │   - REST + WebSocket                               │          
                       │   - SNMP v2c/v3 + traps                            │          
                       │   - Prometheus /metrics                            │          
                       │   - Syslog forwarder (RFC 5424 over TLS)           │          
                       └─────────────────────────┬──────────────────────────┘          
                                                 │  gRPC + mTLS                        
        ─────────────────────────────────────────┼─────────────────────────────────────
                                                 │                                     
                       ┌─────────────────────────▼──────────────────────────┐          
                       │ acm-controller  (Go, central server)               │          
                       │   - PostgreSQL + Keycloak SSO                      │          
                       │   - REST + gRPC                                    │          
                       │   - Fleet orchestration                            │          
                       │   - SvelteKit frontend with i18n (ru / uz)         │          
                       └────────────────────────────────────────────────────┘          
                                                                                       
                       External: Zabbix + Grafana + VictoriaMetrics + Loki             
                                 + Alertmanager — at customer site                     
```

## Iteration plan

| Phase | Crypto | Throughput target | Status |
|---|---|---|---|
| 1 — AES PoC | AES-256-GCM via Marvell SAM | 1 Gbps line-rate | not started |
| 1.5 — Pilot | AES-256-GCM | line-rate, field validation | — |
| 2 — Own 1105 | Rust impl of O'z DSt 1105 + NEON | ~400 Mbps | — |
| 3 — Certified | Licensed Uzbek O'z DSt 1105 library | as above, with sertificate | — |

`AlgoId` is embedded in every wire frame so phases coexist during migration.

## Repository layout

```
cryptod/     Rust workspace — crypto datapath
  crates/
    acm-cryptod    binary
    acm-crypto     CryptoProvider trait + implementations
    acm-wire       ACM encrypted frame format
    acm-ipc        UDS/shmem IPC with the Go agent
    acm-dpdk       FFI to DPDK 25 and MUSDK
agent/       Go module — on-module control plane
  cmd/
    acm-agent      main daemon
    acm-cli        local diagnostics CLI
controller/  Go module — central management server
  cmd/
    acm-controller
proto/       Shared protobuf — CryptoControl, AgentControl
docker/      Builder image (rust + go + node + protoc + aarch64-cross)
docs/
  standards/ O'z DSt 2814–2817:2014 reference PDFs
  vendor/    ISM41xx and ISM4120I user guides
scripts/     Read-only recon and diagnostics helpers
deploy/      systemd units, OTA package templates
```

## Building

All builds run inside a Docker image with the full cross-toolchain.
The only host requirement is Docker + bash (Git Bash on Windows works).

```bash
# First-time setup (downloads ~2-3 GB)
./dev.sh build-image

# Cross-compile everything to dist/
./dev.sh build

# Run unit tests
./dev.sh test

# Format
./dev.sh fmt

# Regenerate Go bindings from proto/
./dev.sh proto

# Interactive shell inside builder
./dev.sh shell
```

Output binaries (after `./dev.sh build`):

| File | Target | Size (typical) |
|---|---|---|
| `dist/acm-cryptod` | aarch64-linux | ~10 MB |
| `dist/acm-agent` | aarch64-linux | ~25 MB (with embedded UI) |
| `dist/acm-cli` | aarch64-linux | ~8 MB |
| `dist/acm-controller` | amd64-linux | ~30 MB |

## Standards reference

The four Uzbek state standards that govern this product are at `docs/standards/`:

- **O'z DSt 2814:2014** — Automated systems, classification by protection level.
- **O'z DSt 2815:2014** — Firewalls, classification (3 classes; we target class 3+ depending on customer profile).
- **O'z DSt 2816:2014** — Software control of absence of undeclared capabilities (НДВ).
- **O'z DSt 2817:2014** — Computing facilities, classification by protection level.

Cryptography itself is defined in **O'z DSt 1105:2009** (block cipher). Hash and
signature are in 1106 and 1092 respectively; not in this repo.

## License

Proprietary — internal R&D, not for distribution.
