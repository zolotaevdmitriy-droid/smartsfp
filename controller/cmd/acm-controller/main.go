// Package main is the ACM-UZ central management controller.
//
// Runs on a dedicated management server (amd64), manages a fleet of
// Smart SFP ISM4120I modules: provisioning, policy distribution, key
// management, telemetry aggregation. Exposes REST + gRPC for the
// SvelteKit web UI; integrates with Keycloak (OIDC), PostgreSQL,
// VictoriaMetrics, Loki, NATS.
//
// Deployed via Docker Compose (or k3s in HA).
package main

import (
	"fmt"
	"runtime"
)

var Version = "0.1.0-dev"

func main() {
	fmt.Printf("ACM-UZ controller %s\n", Version)
	fmt.Printf("  target: %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Println("Hello, ACM-UZ. Controller backend not yet implemented.")

	// TODO Phase 1: PostgreSQL connection, Keycloak OIDC integration,
	// REST/gRPC servers, NATS subscription, fleet manager loop.
}
