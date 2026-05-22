// Package main is the ACM-UZ on-module management agent.
//
// Responsibilities (current iteration):
//   - HTTP server on :9100:
//       /metrics             — Prometheus exposition of cryptod state
//       /api/v1/status       — JSON StatusReport
//       /api/v1/keys/rotate  — POST JSON {key_id, algo, material_hex}
//       /healthz, /readyz    — health probes
//       /                    — minimal web UI (embedded)
//   - UDS client to acm-cryptod for everything above.
//
// Still TODO (next iterations): SNMP, syslog forwarder over TLS,
// SQLite local state, mTLS+OIDC, gRPC to controller, Svelte UI.

package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"runtime"
	"syscall"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/collectors"
	"github.com/prometheus/client_golang/prometheus/promhttp"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/httpapi"
	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/ipc"
	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/metrics"
	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/web"
)

// Version is overridden at build time via -ldflags="-X main.Version=...".
var Version = "0.1.0-dev"

func main() {
	cryptodSock := flag.String(
		"cryptod-socket", ipc.DefaultSocketPath,
		"path to cryptod UDS")
	listenAddr := flag.String(
		"listen", ":9100",
		"address to listen on for HTTP (metrics + API + UI)")
	flag.Parse()

	log.SetFlags(log.LstdFlags | log.Lmicroseconds)
	log.Printf("acm-agent %s starting (target=%s/%s, cryptod=%s, listen=%s)",
		Version, runtime.GOOS, runtime.GOARCH, *cryptodSock, *listenAddr)

	cli := ipc.New(*cryptodSock)
	defer func() { _ = cli.Close() }()

	// Prometheus registry: only the cryptod collector + standard process/Go
	// metrics so /metrics is useful for both fleet ops and Go runtime debug.
	reg := prometheus.NewRegistry()
	reg.MustRegister(metrics.New(cli))
	reg.MustRegister(collectors.NewGoCollector())
	reg.MustRegister(collectors.NewProcessCollector(collectors.ProcessCollectorOpts{
		Namespace: "acm_agent",
	}))

	// HTTP router. One mux, one port: simpler than fanning out, since all
	// endpoints are on the management plane.
	mux := http.NewServeMux()
	mux.Handle("/metrics", promhttp.HandlerFor(reg, promhttp.HandlerOpts{}))
	httpapi.New(cli).Routes(mux)

	// Static UI under /. The embed package lives next to the HTML so
	// `//go:embed` works without ../ paths.
	mux.Handle("/", http.FileServer(http.FS(web.FS())))

	srv := &http.Server{
		Addr:              *listenAddr,
		Handler:           withLogging(mux),
		ReadHeaderTimeout: 10 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       120 * time.Second,
	}

	// Graceful shutdown on SIGTERM/SIGINT.
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	go func() {
		<-ctx.Done()
		log.Printf("shutdown requested, stopping http server")
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdownCtx)
	}()

	log.Printf("listening on %s", *listenAddr)
	if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatalf("http server: %v", err)
	}
	log.Printf("bye")
	os.Exit(0)
}

// withLogging is a minimal access log. Real prod will use structured
// logging + request IDs; this is enough to see what hits us.
func withLogging(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t0 := time.Now()
		rw := &statusRecorder{ResponseWriter: w, status: 200}
		next.ServeHTTP(rw, r)
		log.Printf("%s %s %d %s %s",
			r.Method, r.URL.Path, rw.status,
			fmt.Sprintf("%dms", time.Since(t0).Milliseconds()),
			r.RemoteAddr)
	})
}

type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (s *statusRecorder) WriteHeader(code int) {
	s.status = code
	s.ResponseWriter.WriteHeader(code)
}
