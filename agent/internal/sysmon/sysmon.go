// Package sysmon reads live system metrics from /proc and /sys on Linux.
//
// Designed for the ISM4120I (Cortex-A53 dual-core, Debian 12). All paths
// are pure Linux conventions, no vendor-specific files — same code would
// run on any aarch64 Debian system.
//
// Threadsafe: Reader keeps CPU/network delta state under a mutex so
// multiple HTTP handlers can call Snapshot concurrently.
package sysmon

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
)

// Snapshot is the JSON shape returned to web UI and Prometheus collector.
type Snapshot struct {
	TakenAt     time.Time     `json:"taken_at"`
	UptimeS     float64       `json:"uptime_s"`
	LoadAvg     [3]float64    `json:"load_avg"`
	CPU         CPUInfo       `json:"cpu"`
	Memory      MemoryInfo    `json:"memory"`
	Filesystems []FSInfo      `json:"filesystems"`
	Sensors     []SensorInfo  `json:"sensors"`
	Interfaces  []NetInfo     `json:"interfaces"`
	Processes   []ProcessInfo `json:"processes"`
}

type CPUInfo struct {
	Cores   int       `json:"cores"`
	BusyPct float64   `json:"busy_pct"`        // overall, 0..100
	PerCore []float64 `json:"per_core_pct"`    // [core0, core1, ...]
}

type MemoryInfo struct {
	TotalKB     uint64  `json:"total_kb"`
	AvailableKB uint64  `json:"available_kb"`
	FreeKB      uint64  `json:"free_kb"`
	BuffersKB   uint64  `json:"buffers_kb"`
	CachedKB    uint64  `json:"cached_kb"`
	UsedPct     float64 `json:"used_pct"`        // (total - available) / total
}

type FSInfo struct {
	Mount   string  `json:"mount"`
	SizeKB  uint64  `json:"size_kb"`
	UsedKB  uint64  `json:"used_kb"`
	UsedPct float64 `json:"used_pct"`
}

type SensorInfo struct {
	Name  string  `json:"name"`
	TempC float64 `json:"temp_c"`
}

type NetInfo struct {
	Name       string  `json:"name"`
	Up         bool    `json:"up"`
	MTU        int     `json:"mtu"`
	RxBytes    uint64  `json:"rx_bytes"`
	TxBytes    uint64  `json:"tx_bytes"`
	RxPackets  uint64  `json:"rx_packets"`
	TxPackets  uint64  `json:"tx_packets"`
	RxErrors   uint64  `json:"rx_errors"`
	TxErrors   uint64  `json:"tx_errors"`
	RxDropped  uint64  `json:"rx_dropped"`
	TxDropped  uint64  `json:"tx_dropped"`
	RxBpsNow   float64 `json:"rx_bps_now"` // computed over delta
	TxBpsNow   float64 `json:"tx_bps_now"`
}

// MountsToReport are the filesystems we show on the dashboard. eMMC is
// what matters here — / is read-only by default; /var and /home are
// writable per vendor's partition layout.
var MountsToReport = []string{"/", "/var", "/home"}

// Reader keeps state between Snapshot calls so we can compute CPU% and
// network rates as deltas.
type Reader struct {
	mu       sync.Mutex
	prevCPU  *cpuStat
	prevCPUs map[int]*cpuStat
	prevNet  map[string]netSample
	prevProc map[string]procSample
	prevAt   time.Time
}

func NewReader() *Reader {
	return &Reader{
		prevCPUs: map[int]*cpuStat{},
		prevNet:  map[string]netSample{},
		prevProc: map[string]procSample{},
	}
}

// Snapshot returns a single point-in-time read of all metrics. The first
// call will report CPU% / network rates as 0 because we have no baseline;
// subsequent calls give real deltas.
func (r *Reader) Snapshot() (*Snapshot, error) {
	r.mu.Lock()
	defer r.mu.Unlock()

	s := &Snapshot{TakenAt: time.Now()}

	if up, err := readUptime(); err == nil {
		s.UptimeS = up
	}
	if la, err := readLoadAvg(); err == nil {
		s.LoadAvg = la
	}
	if err := r.readCPU(s); err != nil {
		return nil, fmt.Errorf("cpu: %w", err)
	}
	if err := readMemory(s); err != nil {
		return nil, fmt.Errorf("memory: %w", err)
	}
	s.Filesystems = readFilesystems()
	s.Sensors = readSensors()
	if err := r.readNet(s); err != nil {
		return nil, fmt.Errorf("net: %w", err)
	}
	r.readProcesses(s)
	r.prevAt = s.TakenAt
	return s, nil
}

// ---------------- uptime / loadavg ----------------

func readUptime() (float64, error) {
	b, err := os.ReadFile("/proc/uptime")
	if err != nil {
		return 0, err
	}
	parts := strings.Fields(string(b))
	if len(parts) == 0 {
		return 0, fmt.Errorf("empty /proc/uptime")
	}
	return strconv.ParseFloat(parts[0], 64)
}

func readLoadAvg() ([3]float64, error) {
	var out [3]float64
	b, err := os.ReadFile("/proc/loadavg")
	if err != nil {
		return out, err
	}
	parts := strings.Fields(string(b))
	if len(parts) < 3 {
		return out, fmt.Errorf("bad /proc/loadavg")
	}
	for i := 0; i < 3; i++ {
		v, err := strconv.ParseFloat(parts[i], 64)
		if err != nil {
			return out, err
		}
		out[i] = v
	}
	return out, nil
}

// ---------------- CPU ----------------

// cpuStat holds the cumulative jiffies fields we care about from one
// /proc/stat line. We only need busy vs idle, so collapse the rest.
type cpuStat struct {
	user, nice, system, idle, iowait, irq, softirq, steal uint64
}

func (c cpuStat) total() uint64 {
	return c.user + c.nice + c.system + c.idle + c.iowait + c.irq + c.softirq + c.steal
}
func (c cpuStat) busy() uint64 {
	return c.user + c.nice + c.system + c.irq + c.softirq + c.steal
}

func (r *Reader) readCPU(s *Snapshot) error {
	f, err := os.Open("/proc/stat")
	if err != nil {
		return err
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	var perCore []*cpuStat
	var overall *cpuStat
	for scanner.Scan() {
		line := scanner.Text()
		if !strings.HasPrefix(line, "cpu") {
			break
		}
		parts := strings.Fields(line)
		st := parseCPULine(parts)
		if parts[0] == "cpu" {
			overall = st
		} else {
			perCore = append(perCore, st)
		}
	}
	if overall == nil {
		return fmt.Errorf("no overall cpu line")
	}
	s.CPU.Cores = len(perCore)
	s.CPU.PerCore = make([]float64, len(perCore))

	// Overall %
	if r.prevCPU != nil {
		s.CPU.BusyPct = pctBusy(*r.prevCPU, *overall)
	}
	r.prevCPU = overall

	// Per-core %
	for i, st := range perCore {
		if prev, ok := r.prevCPUs[i]; ok {
			s.CPU.PerCore[i] = pctBusy(*prev, *st)
		}
		r.prevCPUs[i] = st
	}
	return nil
}

func parseCPULine(parts []string) *cpuStat {
	get := func(i int) uint64 {
		if i+1 >= len(parts) {
			return 0
		}
		v, _ := strconv.ParseUint(parts[i+1], 10, 64)
		return v
	}
	return &cpuStat{
		user: get(0), nice: get(1), system: get(2), idle: get(3),
		iowait: get(4), irq: get(5), softirq: get(6), steal: get(7),
	}
}

func pctBusy(prev, cur cpuStat) float64 {
	dt := cur.total() - prev.total()
	db := cur.busy() - prev.busy()
	if dt == 0 {
		return 0
	}
	v := float64(db) * 100.0 / float64(dt)
	if v < 0 {
		v = 0
	}
	if v > 100 {
		v = 100
	}
	return v
}

// ---------------- Memory ----------------

func readMemory(s *Snapshot) error {
	f, err := os.Open("/proc/meminfo")
	if err != nil {
		return err
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		key, val := parseMeminfoLine(scanner.Text())
		switch key {
		case "MemTotal":
			s.Memory.TotalKB = val
		case "MemAvailable":
			s.Memory.AvailableKB = val
		case "MemFree":
			s.Memory.FreeKB = val
		case "Buffers":
			s.Memory.BuffersKB = val
		case "Cached":
			s.Memory.CachedKB = val
		}
	}
	if s.Memory.TotalKB > 0 {
		used := s.Memory.TotalKB - s.Memory.AvailableKB
		s.Memory.UsedPct = float64(used) * 100.0 / float64(s.Memory.TotalKB)
	}
	return nil
}

func parseMeminfoLine(line string) (string, uint64) {
	// "MemTotal:        1010500 kB"
	i := strings.IndexByte(line, ':')
	if i < 0 {
		return "", 0
	}
	key := line[:i]
	rest := strings.TrimSpace(line[i+1:])
	rest = strings.TrimSuffix(rest, " kB")
	v, _ := strconv.ParseUint(strings.TrimSpace(rest), 10, 64)
	return key, v
}

// ---------------- Filesystems ----------------

func readFilesystems() []FSInfo {
	out := make([]FSInfo, 0, len(MountsToReport))
	for _, m := range MountsToReport {
		var st syscall.Statfs_t
		if err := syscall.Statfs(m, &st); err != nil {
			continue
		}
		blockKB := st.Bsize / 1024
		size := uint64(st.Blocks) * uint64(blockKB)
		free := uint64(st.Bavail) * uint64(blockKB)
		used := size - free
		var pct float64
		if size > 0 {
			pct = float64(used) * 100.0 / float64(size)
		}
		out = append(out, FSInfo{
			Mount: m, SizeKB: size, UsedKB: used, UsedPct: pct,
		})
	}
	return out
}

// ---------------- Sensors ----------------

func readSensors() []SensorInfo {
	matches, err := filepath.Glob("/sys/class/hwmon/hwmon*/name")
	if err != nil {
		return nil
	}
	var out []SensorInfo
	for _, np := range matches {
		dir := filepath.Dir(np)
		name := strings.TrimSpace(readString(np))
		// Many hwmon devices expose temp1_input; some also expose temp2 etc.
		// For our minimal monitoring we just take temp1_input.
		tempPath := filepath.Join(dir, "temp1_input")
		raw := strings.TrimSpace(readString(tempPath))
		if raw == "" {
			continue
		}
		mC, err := strconv.ParseInt(raw, 10, 64)
		if err != nil {
			continue
		}
		out = append(out, SensorInfo{
			Name: name, TempC: float64(mC) / 1000.0,
		})
	}
	return out
}

func readString(p string) string {
	b, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	return string(b)
}

// ---------------- Network ----------------

type netSample struct {
	at      time.Time
	rxBytes uint64
	txBytes uint64
}

// Interfaces we want to surface on the dashboard. Anything else is
// skipped — saves the UI from clutter (lo, dummy0, bond0, sit0, ...).
var netInterfaces = []string{"gbe0", "gbe1", "br0"}

func (r *Reader) readNet(s *Snapshot) error {
	stats, err := parseProcNetDev()
	if err != nil {
		return err
	}
	sort.Strings(netInterfaces)
	for _, name := range netInterfaces {
		st, ok := stats[name]
		if !ok {
			continue
		}
		up := readString(filepath.Join("/sys/class/net", name, "operstate"))
		mtu, _ := strconv.Atoi(strings.TrimSpace(readString(filepath.Join("/sys/class/net", name, "mtu"))))
		info := NetInfo{
			Name:      name,
			Up:        strings.TrimSpace(up) == "up",
			MTU:       mtu,
			RxBytes:   st.rxBytes,
			TxBytes:   st.txBytes,
			RxPackets: st.rxPackets,
			TxPackets: st.txPackets,
			RxErrors:  st.rxErrors,
			TxErrors:  st.txErrors,
			RxDropped: st.rxDropped,
			TxDropped: st.txDropped,
		}
		if prev, ok := r.prevNet[name]; ok {
			dt := s.TakenAt.Sub(prev.at).Seconds()
			if dt > 0.05 {
				if st.rxBytes >= prev.rxBytes {
					info.RxBpsNow = float64(st.rxBytes-prev.rxBytes) * 8.0 / dt
				}
				if st.txBytes >= prev.txBytes {
					info.TxBpsNow = float64(st.txBytes-prev.txBytes) * 8.0 / dt
				}
			}
		}
		r.prevNet[name] = netSample{at: s.TakenAt, rxBytes: st.rxBytes, txBytes: st.txBytes}
		s.Interfaces = append(s.Interfaces, info)
	}
	return nil
}

type rawNet struct {
	rxBytes, rxPackets, rxErrors, rxDropped uint64
	txBytes, txPackets, txErrors, txDropped uint64
}

// /proc/net/dev columns:
// Inter-|   Receive                                                |  Transmit
//  face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
func parseProcNetDev() (map[string]rawNet, error) {
	f, err := os.Open("/proc/net/dev")
	if err != nil {
		return nil, err
	}
	defer f.Close()
	out := make(map[string]rawNet)
	scanner := bufio.NewScanner(f)
	lineNum := 0
	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		if lineNum <= 2 {
			continue
		}
		i := strings.IndexByte(line, ':')
		if i < 0 {
			continue
		}
		name := strings.TrimSpace(line[:i])
		fields := strings.Fields(line[i+1:])
		if len(fields) < 16 {
			continue
		}
		u := func(s string) uint64 { v, _ := strconv.ParseUint(s, 10, 64); return v }
		out[name] = rawNet{
			rxBytes:   u(fields[0]),
			rxPackets: u(fields[1]),
			rxErrors:  u(fields[2]),
			rxDropped: u(fields[3]),
			txBytes:   u(fields[8]),
			txPackets: u(fields[9]),
			txErrors:  u(fields[10]),
			txDropped: u(fields[11]),
		}
	}
	return out, nil
}
