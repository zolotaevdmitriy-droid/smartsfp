// Package main is the ACM-UZ on-module management agent.
//
// Responsibilities:
//   - Local web UI (Svelte SPA, embedded via go:embed) + REST/WebSocket on :443.
//   - SNMP v2c/v3 agent + traps to the customer NMS.
//   - Prometheus /metrics exporter for the central monitoring stack.
//   - Syslog forwarder (RFC 5424 over TLS) for audit logs.
//   - gRPC client to the central controller — reverse channel for
//     provisioning, policy push, key rotation, telemetry stream.
//   - Local IPC (Unix Domain Socket) to acm-cryptod for control plane;
//     shared memory ring read for high-rate stats.
//   - SQLite-backed local state (audit log, last-known policy, cached
//     controller fingerprint).
//
// Runs as a dedicated `acm` user, pinned to CPU core 0 via systemd, with
// minimal capabilities (CAP_NET_BIND_SERVICE for low ports only).
package main

import (
	"fmt"
	"os"
	"runtime"
)

// Version is set at build time via -ldflags="-X main.Version=...".
var Version = "0.1.0-dev"

func main() {
	fmt.Printf("ACM-UZ agent %s\n", Version)
	fmt.Printf("  target: %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Printf("  go:     %s\n", runtime.Version())
	fmt.Println()
	fmt.Println("Hello, ACM-UZ. Agent control plane not yet implemented.")

	// TODO Phase 1: load /etc/acm/agent.yaml, open SQLite state, start HTTP
	// server with embedded Svelte UI, start SNMP agent, start Prometheus
	// exporter, dial cryptod UDS, dial controller gRPC.

	os.Exit(0)
}
