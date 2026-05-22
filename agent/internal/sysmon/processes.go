// Process monitoring for our own binaries (acm-cryptod, acm-agent).
//
// Strategy:
//   - WatchedProcesses lists the comm names we care about.
//   - On every Snapshot we walk /proc, match by /proc/[pid]/comm (truncated
//     to 15 chars by the kernel — same limit applies to our matcher).
//   - For each match, parse /proc/[pid]/stat + /proc/[pid]/status, follow
//     /proc/[pid]/exe to find the binary path, then stat it on disk.
//   - CPU% is delta (utime+stime) / wall-time, computed against the prev
//     sample stored in Reader.

package sysmon

import (
	"bufio"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

// WatchedProcesses are the binary names we monitor. Kernel truncates
// /proc/[pid]/comm to 15 chars so our patterns must be short enough.
var WatchedProcesses = []string{"acm-cryptod", "acm-agent"}

// ProcessInfo is what we report via /api/v1/system.
type ProcessInfo struct {
	Name             string  `json:"name"`
	Running          bool    `json:"running"`
	PID              int     `json:"pid"`
	RSSKB            uint64  `json:"rss_kb"`
	CPUPct           float64 `json:"cpu_pct"`
	Threads          int     `json:"threads"`
	StartUnixS       int64   `json:"start_unix_s"`
	UptimeS          float64 `json:"uptime_s"`
	State            string  `json:"state"`
	ExePath          string  `json:"exe_path"`           // from /proc/[pid]/exe symlink
	BinarySize       int64   `json:"binary_size"`        // size of exe on disk
	BinaryMtimeUnixS int64   `json:"binary_mtime_unix_s"`
}

// procStat is a parsed subset of /proc/[pid]/stat we care about.
type procStat struct {
	state     string
	utime     uint64 // jiffies
	stime     uint64
	threads   int
	starttime uint64 // jiffies since boot
}

type procSample struct {
	at        time.Time
	utimePlus uint64 // utime + stime in jiffies
}

// Append process snapshots to s.Processes (called by Snapshot).
func (r *Reader) readProcesses(s *Snapshot) {
	out := make([]ProcessInfo, 0, len(WatchedProcesses))
	for _, name := range WatchedProcesses {
		out = append(out, r.readOneProcess(name, s.TakenAt))
	}
	s.Processes = out
}

func (r *Reader) readOneProcess(name string, at time.Time) ProcessInfo {
	info := ProcessInfo{Name: name}
	pid, found := findPIDByComm(name)
	if !found {
		// Process not running. Still report binary on disk so UI can show
		// "deployed but not running" state.
		info.ExePath, info.BinarySize, info.BinaryMtimeUnixS = guessBinaryOnDisk(name)
		return info
	}
	info.PID = pid
	info.Running = true

	if st, ok := parseProcPidStat(pid); ok {
		info.State = st.state
		info.Threads = st.threads

		// Start time: kernel jiffies since boot. Convert to unix seconds.
		// CLK_TCK is virtually always 100 on Linux; we read it once.
		hz := clockTicks()
		bootUnix := bootTimeUnix()
		info.StartUnixS = bootUnix + int64(st.starttime)/int64(hz)
		info.UptimeS = float64(time.Now().Unix() - info.StartUnixS)

		// CPU%: (delta utime+stime) / (delta wallclock) / hz * 100.
		key := "pid-" + strconv.Itoa(pid)
		cur := st.utime + st.stime
		if prev, ok := r.prevProc[key]; ok && prev.at.Before(at) {
			dt := at.Sub(prev.at).Seconds()
			if dt > 0.05 && cur >= prev.utimePlus {
				dj := float64(cur - prev.utimePlus)
				info.CPUPct = dj * 100.0 / (dt * float64(hz))
				if info.CPUPct < 0 {
					info.CPUPct = 0
				}
			}
		}
		r.prevProc[key] = procSample{at: at, utimePlus: cur}
	}

	if rss, ok := parseProcPidStatusVmRSS(pid); ok {
		info.RSSKB = rss
	}

	if exe, err := os.Readlink("/proc/" + strconv.Itoa(pid) + "/exe"); err == nil {
		info.ExePath = exe
		if fi, err := os.Stat(exe); err == nil {
			info.BinarySize = fi.Size()
			info.BinaryMtimeUnixS = fi.ModTime().Unix()
		}
	}
	return info
}

// findPIDByComm scans /proc and returns the first PID whose comm matches.
// Kernel limits comm to TASK_COMM_LEN-1 = 15 chars. Names longer than that
// will simply not match — currently our binary names are short enough.
func findPIDByComm(name string) (int, bool) {
	entries, err := os.ReadDir("/proc")
	if err != nil {
		return 0, false
	}
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		pid, err := strconv.Atoi(e.Name())
		if err != nil {
			continue
		}
		comm, err := os.ReadFile("/proc/" + e.Name() + "/comm")
		if err != nil {
			continue
		}
		if strings.TrimSpace(string(comm)) == name {
			return pid, true
		}
	}
	return 0, false
}

// parseProcPidStat reads /proc/[pid]/stat. The "comm" field is in
// parentheses and may contain whitespace, so we find the LAST ')' to
// safely split.
func parseProcPidStat(pid int) (procStat, bool) {
	b, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return procStat{}, false
	}
	line := string(b)
	rparen := strings.LastIndexByte(line, ')')
	if rparen < 0 {
		return procStat{}, false
	}
	// After "...) " — fields 3..n. So field 3 is at parts[0] below.
	rest := strings.TrimSpace(line[rparen+1:])
	parts := strings.Fields(rest)
	// rest field indices (1-based in proc(5)) → 0-based in parts:
	//   3 state       → parts[0]
	//   14 utime      → parts[11]
	//   15 stime      → parts[12]
	//   20 num_threads → parts[17]
	//   22 starttime  → parts[19]
	if len(parts) < 20 {
		return procStat{}, false
	}
	atou := func(s string) uint64 { v, _ := strconv.ParseUint(s, 10, 64); return v }
	atoi := func(s string) int { v, _ := strconv.Atoi(s); return v }
	return procStat{
		state:     parts[0],
		utime:     atou(parts[11]),
		stime:     atou(parts[12]),
		threads:   atoi(parts[17]),
		starttime: atou(parts[19]),
	}, true
}

func parseProcPidStatusVmRSS(pid int) (uint64, bool) {
	f, err := os.Open("/proc/" + strconv.Itoa(pid) + "/status")
	if err != nil {
		return 0, false
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		k, v := parseMeminfoLine(scanner.Text())
		if k == "VmRSS" {
			return v, true
		}
	}
	return 0, false
}

// guessBinaryOnDisk tries the deployment directory we use during dev.
// In production with proper systemd install, this would be /usr/local/bin/.
// Both paths are tried.
var binaryPathCandidates = []string{
	"/home/user/acm-uz/",
	"/usr/local/bin/",
}

func guessBinaryOnDisk(name string) (path string, size int64, mtime int64) {
	for _, dir := range binaryPathCandidates {
		p := filepath.Join(dir, name)
		fi, err := os.Stat(p)
		if err != nil {
			continue
		}
		return p, fi.Size(), fi.ModTime().Unix()
	}
	return "", 0, 0
}

// ---- platform constants ----

// CLK_TCK is normally 100 on Linux; read once via sysconf(_SC_CLK_TCK).
// We cache it because the value never changes within a process.
var (
	clkTckCached   int64
	bootTimeCached int64
)

func clockTicks() int64 {
	if clkTckCached != 0 {
		return clkTckCached
	}
	// syscall.Sysconf isn't in the stdlib — but we can read it via
	// /proc/self/auxv or just default to 100. Linux has hardcoded 100
	// for a long time on all major arches.
	clkTckCached = 100
	return clkTckCached
}

func bootTimeUnix() int64 {
	if bootTimeCached != 0 {
		return bootTimeCached
	}
	f, err := os.Open("/proc/stat")
	if err != nil {
		return 0
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := scanner.Text()
		if strings.HasPrefix(line, "btime ") {
			n, _ := strconv.ParseInt(strings.TrimSpace(line[6:]), 10, 64)
			bootTimeCached = n
			return n
		}
	}
	return 0
}

// Silence "unused" warning for syscall import if removed by mistake.
var _ = syscall.Stat_t{}
