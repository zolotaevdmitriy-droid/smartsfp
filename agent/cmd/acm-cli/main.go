// Package main is the ACM-UZ local diagnostics CLI, intended for the
// rare cases when an operator SSHes into the module directly: rescue,
// preflight checks before starting cryptod, manual policy inspection.
package main

import (
	"fmt"
	"runtime"
)

var Version = "0.1.0-dev"

func main() {
	fmt.Printf("acm-cli %s (%s/%s)\n", Version, runtime.GOOS, runtime.GOARCH)
	fmt.Println("Hello, ACM-UZ. CLI subcommands not yet implemented.")

	// TODO Phase 1: health, status, dump-keys (masked), preflight,
	// dump-policies, factory-reset --confirm.
}
