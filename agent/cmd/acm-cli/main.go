// Package main is the ACM-UZ local diagnostics CLI, intended for the
// rare cases when an operator SSHes into the module directly: rescue,
// preflight checks before starting cryptod, manual policy inspection.
//
// Subcommands:
//
//	acm-cli status                                 — GetStatus to cryptod
//	acm-cli rotate-key <id> <algo> <hex-material>  — RotateKey
//
// All commands accept --socket=<path> (default /run/acm/cryptod.sock).
package main

import (
	"context"
	"encoding/hex"
	"flag"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"time"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/ipc"
)

var Version = "0.1.0-dev"

func usage() {
	fmt.Fprintf(os.Stderr, `acm-cli %s (%s/%s)

Usage:
  acm-cli [--socket=PATH] status
  acm-cli [--socket=PATH] rotate-key KEY_ID ALGO HEX_MATERIAL

Options:
  --socket=PATH   UDS to cryptod (default: %s)

ALGO is the AlgoId byte (decimal or 0x-prefixed hex):
  1   AES-128-GCM   key=16 bytes
  2   AES-256-GCM   key=32 bytes
  16  O'z DSt 1105  key=32 bytes (not yet supported)

Example:
  acm-cli rotate-key 7 2 $(openssl rand -hex 32)
`, Version, runtime.GOOS, runtime.GOARCH, ipc.DefaultSocketPath)
}

func main() {
	socket := flag.String("socket", ipc.DefaultSocketPath, "cryptod UDS path")
	flag.Usage = usage
	flag.Parse()

	args := flag.Args()
	if len(args) == 0 {
		usage()
		os.Exit(1)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	cli := ipc.New(*socket)
	defer func() { _ = cli.Close() }()

	switch args[0] {
	case "status":
		if len(args) != 1 {
			fmt.Fprintln(os.Stderr, "status takes no args")
			os.Exit(2)
		}
		st, err := cli.GetStatus(ctx)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(3)
		}
		printStatus(st)

	case "rotate-key":
		if len(args) != 4 {
			fmt.Fprintln(os.Stderr, "rotate-key takes KEY_ID ALGO HEX_MATERIAL")
			os.Exit(2)
		}
		keyID, err := strconv.ParseUint(args[1], 0, 32)
		if err != nil {
			fmt.Fprintf(os.Stderr, "bad KEY_ID: %v\n", err)
			os.Exit(2)
		}
		algo, err := strconv.ParseUint(args[2], 0, 8)
		if err != nil {
			fmt.Fprintf(os.Stderr, "bad ALGO: %v\n", err)
			os.Exit(2)
		}
		material, err := hex.DecodeString(args[3])
		if err != nil {
			fmt.Fprintf(os.Stderr, "bad HEX_MATERIAL: %v\n", err)
			os.Exit(2)
		}
		if err := cli.RotateKey(ctx, uint32(keyID), uint8(algo), material); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(3)
		}
		fmt.Println("ok")

	default:
		fmt.Fprintf(os.Stderr, "unknown subcommand: %s\n\n", args[0])
		usage()
		os.Exit(2)
	}
}

func printStatus(s *ipc.StatusReport) {
	fmt.Printf("version:          %s\n", s.Version)
	fmt.Printf("running:          %t\n", s.Running)
	fmt.Printf("uptime_s:         %d\n", s.UptimeS)
	fmt.Printf("active_provider:  %s\n", s.ActiveProvider)
	if s.ActiveKeyID != nil {
		fmt.Printf("active_key_id:    %d\n", *s.ActiveKeyID)
	} else {
		fmt.Printf("active_key_id:    (none)\n")
	}
	fmt.Printf("packets_sealed:   %d\n", s.PacketsSealed)
	fmt.Printf("packets_opened:   %d\n", s.PacketsOpened)
	fmt.Printf("crypto_errors:    %d\n", s.CryptoErrors)
}
