package metrics

import (
	"github.com/prometheus/client_golang/prometheus"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/sysmon"
)

// SystemCollector exposes sysmon snapshots as Prometheus metrics in the
// `acm_module_*` namespace. Pull-pattern: every scrape calls Snapshot.
type SystemCollector struct {
	sys *sysmon.Reader

	uptime       *prometheus.Desc
	loadavg      *prometheus.Desc
	cpuBusy      *prometheus.Desc
	cpuPerCore   *prometheus.Desc
	memTotal     *prometheus.Desc
	memAvail     *prometheus.Desc
	memUsedPct   *prometheus.Desc
	fsSize       *prometheus.Desc
	fsUsed       *prometheus.Desc
	fsUsedPct    *prometheus.Desc
	tempC        *prometheus.Desc
	ifUp         *prometheus.Desc
	ifRxBytes    *prometheus.Desc
	ifTxBytes    *prometheus.Desc
	ifRxPackets  *prometheus.Desc
	ifTxPackets  *prometheus.Desc
	ifRxErrors   *prometheus.Desc
	ifTxErrors   *prometheus.Desc
	ifRxBpsNow   *prometheus.Desc
	ifTxBpsNow   *prometheus.Desc

	procUp           *prometheus.Desc
	procRSS          *prometheus.Desc
	procCPUPct       *prometheus.Desc
	procThreads      *prometheus.Desc
	procStartTime    *prometheus.Desc
	procUptime       *prometheus.Desc
	procBinarySize   *prometheus.Desc
	procBinaryMtime  *prometheus.Desc
}

func NewSystem(sys *sysmon.Reader) *SystemCollector {
	const ns = "acm_module"
	return &SystemCollector{
		sys: sys,
		uptime: prometheus.NewDesc(ns+"_uptime_seconds",
			"Module uptime in seconds (from /proc/uptime).", nil, nil),
		loadavg: prometheus.NewDesc(ns+"_load_average",
			"Load average. Period label: 1m / 5m / 15m.",
			[]string{"period"}, nil),
		cpuBusy: prometheus.NewDesc(ns+"_cpu_busy_pct",
			"Overall CPU busy percentage since the last scrape (0..100).",
			nil, nil),
		cpuPerCore: prometheus.NewDesc(ns+"_cpu_core_busy_pct",
			"Per-core CPU busy percentage since the last scrape (0..100).",
			[]string{"core"}, nil),
		memTotal: prometheus.NewDesc(ns+"_memory_total_bytes",
			"Total RAM, bytes.", nil, nil),
		memAvail: prometheus.NewDesc(ns+"_memory_available_bytes",
			"Available RAM, bytes.", nil, nil),
		memUsedPct: prometheus.NewDesc(ns+"_memory_used_pct",
			"RAM utilization in percent (0..100).", nil, nil),
		fsSize: prometheus.NewDesc(ns+"_filesystem_size_bytes",
			"Filesystem total size.", []string{"mount"}, nil),
		fsUsed: prometheus.NewDesc(ns+"_filesystem_used_bytes",
			"Filesystem used bytes.", []string{"mount"}, nil),
		fsUsedPct: prometheus.NewDesc(ns+"_filesystem_used_pct",
			"Filesystem utilization in percent.", []string{"mount"}, nil),
		tempC: prometheus.NewDesc(ns+"_temperature_celsius",
			"Temperature sensor reading in C.", []string{"sensor"}, nil),
		ifUp: prometheus.NewDesc(ns+"_interface_up",
			"Network interface operational state (1 up, 0 down).",
			[]string{"iface"}, nil),
		ifRxBytes: prometheus.NewDesc(ns+"_interface_rx_bytes_total",
			"Cumulative bytes received per interface.",
			[]string{"iface"}, nil),
		ifTxBytes: prometheus.NewDesc(ns+"_interface_tx_bytes_total",
			"Cumulative bytes transmitted per interface.",
			[]string{"iface"}, nil),
		ifRxPackets: prometheus.NewDesc(ns+"_interface_rx_packets_total",
			"Cumulative packets received per interface.",
			[]string{"iface"}, nil),
		ifTxPackets: prometheus.NewDesc(ns+"_interface_tx_packets_total",
			"Cumulative packets transmitted per interface.",
			[]string{"iface"}, nil),
		ifRxErrors: prometheus.NewDesc(ns+"_interface_rx_errors_total",
			"Cumulative receive errors per interface.",
			[]string{"iface"}, nil),
		ifTxErrors: prometheus.NewDesc(ns+"_interface_tx_errors_total",
			"Cumulative transmit errors per interface.",
			[]string{"iface"}, nil),
		ifRxBpsNow: prometheus.NewDesc(ns+"_interface_rx_bps",
			"Recent receive rate, bits per second (computed delta).",
			[]string{"iface"}, nil),
		ifTxBpsNow: prometheus.NewDesc(ns+"_interface_tx_bps",
			"Recent transmit rate, bits per second (computed delta).",
			[]string{"iface"}, nil),

		procUp: prometheus.NewDesc("acm_process_up",
			"Whether one of our binaries is running (1=running, 0=down).",
			[]string{"name"}, nil),
		procRSS: prometheus.NewDesc("acm_process_memory_rss_bytes",
			"Resident set size of the process.",
			[]string{"name"}, nil),
		procCPUPct: prometheus.NewDesc("acm_process_cpu_percent",
			"CPU percent computed over the last scrape interval (0..100 per core).",
			[]string{"name"}, nil),
		procThreads: prometheus.NewDesc("acm_process_threads",
			"Number of threads in the process.",
			[]string{"name"}, nil),
		procStartTime: prometheus.NewDesc("acm_process_start_time_seconds",
			"Process start time, unix seconds.",
			[]string{"name"}, nil),
		procUptime: prometheus.NewDesc("acm_process_uptime_seconds",
			"Wall-clock seconds since process start.",
			[]string{"name"}, nil),
		procBinarySize: prometheus.NewDesc("acm_process_binary_size_bytes",
			"Size of the on-disk binary backing the process.",
			[]string{"name", "path"}, nil),
		procBinaryMtime: prometheus.NewDesc("acm_process_binary_mtime_seconds",
			"Modification time of the on-disk binary (unix seconds).",
			[]string{"name", "path"}, nil),
	}
}

func (c *SystemCollector) Describe(ch chan<- *prometheus.Desc) {
	ch <- c.uptime
	ch <- c.loadavg
	ch <- c.cpuBusy
	ch <- c.cpuPerCore
	ch <- c.memTotal
	ch <- c.memAvail
	ch <- c.memUsedPct
	ch <- c.fsSize
	ch <- c.fsUsed
	ch <- c.fsUsedPct
	ch <- c.tempC
	ch <- c.ifUp
	ch <- c.ifRxBytes
	ch <- c.ifTxBytes
	ch <- c.ifRxPackets
	ch <- c.ifTxPackets
	ch <- c.ifRxErrors
	ch <- c.ifTxErrors
	ch <- c.ifRxBpsNow
	ch <- c.ifTxBpsNow
	ch <- c.procUp
	ch <- c.procRSS
	ch <- c.procCPUPct
	ch <- c.procThreads
	ch <- c.procStartTime
	ch <- c.procUptime
	ch <- c.procBinarySize
	ch <- c.procBinaryMtime
}

func (c *SystemCollector) Collect(ch chan<- prometheus.Metric) {
	snap, err := c.sys.Snapshot()
	if err != nil {
		return
	}
	g := prometheus.GaugeValue
	cnt := prometheus.CounterValue

	ch <- prometheus.MustNewConstMetric(c.uptime, g, snap.UptimeS)
	periods := [3]string{"1m", "5m", "15m"}
	for i, p := range periods {
		ch <- prometheus.MustNewConstMetric(c.loadavg, g, snap.LoadAvg[i], p)
	}
	ch <- prometheus.MustNewConstMetric(c.cpuBusy, g, snap.CPU.BusyPct)
	for i, pct := range snap.CPU.PerCore {
		ch <- prometheus.MustNewConstMetric(c.cpuPerCore, g, pct,
			itoa(i))
	}
	ch <- prometheus.MustNewConstMetric(c.memTotal, g,
		float64(snap.Memory.TotalKB)*1024)
	ch <- prometheus.MustNewConstMetric(c.memAvail, g,
		float64(snap.Memory.AvailableKB)*1024)
	ch <- prometheus.MustNewConstMetric(c.memUsedPct, g, snap.Memory.UsedPct)
	for _, fs := range snap.Filesystems {
		ch <- prometheus.MustNewConstMetric(c.fsSize, g,
			float64(fs.SizeKB)*1024, fs.Mount)
		ch <- prometheus.MustNewConstMetric(c.fsUsed, g,
			float64(fs.UsedKB)*1024, fs.Mount)
		ch <- prometheus.MustNewConstMetric(c.fsUsedPct, g,
			fs.UsedPct, fs.Mount)
	}
	for _, s := range snap.Sensors {
		ch <- prometheus.MustNewConstMetric(c.tempC, g, s.TempC, s.Name)
	}
	for _, ni := range snap.Interfaces {
		up := 0.0
		if ni.Up {
			up = 1.0
		}
		ch <- prometheus.MustNewConstMetric(c.ifUp, g, up, ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifRxBytes, cnt,
			float64(ni.RxBytes), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifTxBytes, cnt,
			float64(ni.TxBytes), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifRxPackets, cnt,
			float64(ni.RxPackets), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifTxPackets, cnt,
			float64(ni.TxPackets), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifRxErrors, cnt,
			float64(ni.RxErrors), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifTxErrors, cnt,
			float64(ni.TxErrors), ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifRxBpsNow, g,
			ni.RxBpsNow, ni.Name)
		ch <- prometheus.MustNewConstMetric(c.ifTxBpsNow, g,
			ni.TxBpsNow, ni.Name)
	}

	for _, p := range snap.Processes {
		up := 0.0
		if p.Running {
			up = 1.0
		}
		ch <- prometheus.MustNewConstMetric(c.procUp, g, up, p.Name)
		if p.Running {
			ch <- prometheus.MustNewConstMetric(c.procRSS, g,
				float64(p.RSSKB)*1024, p.Name)
			ch <- prometheus.MustNewConstMetric(c.procCPUPct, g,
				p.CPUPct, p.Name)
			ch <- prometheus.MustNewConstMetric(c.procThreads, g,
				float64(p.Threads), p.Name)
			ch <- prometheus.MustNewConstMetric(c.procStartTime, g,
				float64(p.StartUnixS), p.Name)
			ch <- prometheus.MustNewConstMetric(c.procUptime, g,
				p.UptimeS, p.Name)
		}
		if p.BinarySize > 0 {
			path := p.ExePath
			if path == "" {
				path = "(unknown)"
			}
			ch <- prometheus.MustNewConstMetric(c.procBinarySize, g,
				float64(p.BinarySize), p.Name, path)
			ch <- prometheus.MustNewConstMetric(c.procBinaryMtime, g,
				float64(p.BinaryMtimeUnixS), p.Name, path)
		}
	}
}

func itoa(n int) string {
	// fmt.Sprintf would work but is overkill for small numbers in hot path.
	if n == 0 { return "0" }
	if n < 10 { return string(rune('0' + n)) }
	// Two-digit fallback; we never have more than a few cores.
	tens := n / 10
	ones := n % 10
	return string([]rune{rune('0' + tens), rune('0' + ones)})
}
