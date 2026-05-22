// Package metrics is the Prometheus collector that exports cryptod state
// over /metrics. Pull-based: each scrape opens (or reuses) the UDS to
// cryptod, fetches StatusReport, and emits the corresponding metrics.
//
// If cryptod is unreachable, only `acm_cryptod_up{} = 0` is emitted —
// Prometheus / Zabbix can alert on that.
package metrics

import (
	"context"
	"time"

	"github.com/prometheus/client_golang/prometheus"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/ipc"
)

// CryptodCollector turns cryptod IPC state into Prometheus metrics.
type CryptodCollector struct {
	cli *ipc.Client

	up               *prometheus.Desc
	uptimeSeconds    *prometheus.Desc
	versionInfo      *prometheus.Desc
	activeKeyID      *prometheus.Desc
	packetsSealed    *prometheus.Desc
	packetsOpened    *prometheus.Desc
	cryptoErrors     *prometheus.Desc
	ipcScrapeLatency *prometheus.Desc
}

// New builds a collector against the given UDS client (caller manages
// the client's lifetime).
func New(cli *ipc.Client) *CryptodCollector {
	const ns = "acm_cryptod"
	return &CryptodCollector{
		cli: cli,
		up: prometheus.NewDesc(
			ns+"_up",
			"1 if the local cryptod responded to the last IPC GetStatus, 0 otherwise.",
			nil, nil,
		),
		uptimeSeconds: prometheus.NewDesc(
			ns+"_uptime_seconds",
			"Time since cryptod process started, seconds.",
			nil, nil,
		),
		versionInfo: prometheus.NewDesc(
			ns+"_version_info",
			"Build / runtime info as labels; value is always 1. Use group_left to join.",
			[]string{"version", "active_provider"}, nil,
		),
		activeKeyID: prometheus.NewDesc(
			ns+"_active_key_id",
			"Currently active key id in cryptod. -1 if no key is set.",
			nil, nil,
		),
		packetsSealed: prometheus.NewDesc(
			ns+"_packets_sealed_total",
			"Total packets successfully sealed (encrypt+tag) through cryptod.",
			nil, nil,
		),
		packetsOpened: prometheus.NewDesc(
			ns+"_packets_opened_total",
			"Total packets successfully opened (decrypt+verify) through cryptod.",
			nil, nil,
		),
		cryptoErrors: prometheus.NewDesc(
			ns+"_errors_total",
			"Total cryptographic operations that failed (bad tag, malformed frame, ...).",
			nil, nil,
		),
		ipcScrapeLatency: prometheus.NewDesc(
			ns+"_ipc_scrape_seconds",
			"How long the most recent IPC GetStatus took, seconds.",
			nil, nil,
		),
	}
}

// Describe implements prometheus.Collector.
func (c *CryptodCollector) Describe(ch chan<- *prometheus.Desc) {
	ch <- c.up
	ch <- c.uptimeSeconds
	ch <- c.versionInfo
	ch <- c.activeKeyID
	ch <- c.packetsSealed
	ch <- c.packetsOpened
	ch <- c.cryptoErrors
	ch <- c.ipcScrapeLatency
}

// Collect implements prometheus.Collector. One IPC round-trip per scrape.
func (c *CryptodCollector) Collect(ch chan<- prometheus.Metric) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	t0 := time.Now()
	st, err := c.cli.GetStatus(ctx)
	scrapeSec := time.Since(t0).Seconds()
	ch <- prometheus.MustNewConstMetric(c.ipcScrapeLatency, prometheus.GaugeValue, scrapeSec)

	if err != nil {
		ch <- prometheus.MustNewConstMetric(c.up, prometheus.GaugeValue, 0)
		return
	}

	ch <- prometheus.MustNewConstMetric(c.up, prometheus.GaugeValue, 1)
	ch <- prometheus.MustNewConstMetric(c.uptimeSeconds, prometheus.GaugeValue, float64(st.UptimeS))
	ch <- prometheus.MustNewConstMetric(c.versionInfo, prometheus.GaugeValue, 1, st.Version, st.ActiveProvider)
	if st.ActiveKeyID != nil {
		ch <- prometheus.MustNewConstMetric(c.activeKeyID, prometheus.GaugeValue, float64(*st.ActiveKeyID))
	} else {
		ch <- prometheus.MustNewConstMetric(c.activeKeyID, prometheus.GaugeValue, -1)
	}
	ch <- prometheus.MustNewConstMetric(c.packetsSealed, prometheus.CounterValue, float64(st.PacketsSealed))
	ch <- prometheus.MustNewConstMetric(c.packetsOpened, prometheus.CounterValue, float64(st.PacketsOpened))
	ch <- prometheus.MustNewConstMetric(c.cryptoErrors, prometheus.CounterValue, float64(st.CryptoErrors))
}
