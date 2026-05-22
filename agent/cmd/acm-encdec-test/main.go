// Package main is a one-shot E2E sanity test for acm-cryptod's
// Encrypt/Decrypt IPC methods.
//
// Run it on the module after starting cryptod with --ipc-socket. It will:
//
//	1. Read --socket UDS.
//	2. status — must show some active key (caller is responsible for
//	   having done rotate-key first).
//	3. Encrypt N test plaintexts of varying sizes, Decrypt each one back,
//	   assert plaintext matches.
//	4. Tamper a ciphertext byte, Decrypt -> expect error code 401.
//	5. Final status — packets_sealed / packets_opened should have grown,
//	   crypto_errors should have grown by exactly 1.
//	6. Print PASS / FAIL summary.
//
// Exit code 0 = all asserts passed, non-zero = something broke.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"errors"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/ipc"
)

func main() {
	socket := flag.String("socket", "/tmp/acm/cryptod.sock", "cryptod UDS")
	flag.Parse()

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	cli := ipc.New(*socket)
	defer cli.Close()

	// 1. Initial status — for the report.
	st, err := cli.GetStatus(ctx)
	if err != nil {
		fail("initial status: %v", err)
	}
	keyStr := "(none)"
	if st.ActiveKeyID != nil {
		keyStr = fmt.Sprintf("%d", *st.ActiveKeyID)
	}
	fmt.Printf("BEFORE: sealed=%d opened=%d errors=%d key=%s\n",
		st.PacketsSealed, st.PacketsOpened, st.CryptoErrors, keyStr)
	if st.ActiveKeyID == nil {
		fail("no active key — run `acm-cli rotate-key <id> 2 <hexkey>` first")
	}

	sizes := []int{0, 1, 15, 16, 17, 64, 256, 1500, 9000}
	for _, n := range sizes {
		pt := randBytes(n)
		nonce := randBytes(12) // unique per call — safe per (key, nonce)
		aad := []byte("acm-uz-e2e")
		frame, err := cli.Encrypt(ctx, nonce, aad, pt)
		if err != nil {
			fail("Encrypt(size=%d): %v", n, err)
		}
		if n+12+12+16 != len(frame) {
			// header 12 + nonce 12 + ct (==pt) + tag 16
			fail("frame size mismatch: pt=%d frame=%d (want %d)", n, len(frame), n+12+12+16)
		}
		back, err := cli.Decrypt(ctx, frame)
		if err != nil {
			fail("Decrypt(size=%d): %v", n, err)
		}
		if !bytes.Equal(back, pt) {
			fail("roundtrip mismatch at size %d", n)
		}
		fmt.Printf("  ok roundtrip size=%d frame=%dB\n", n, len(frame))
	}

	// Tamper test: flip a ciphertext byte (skip the 12B header + 12B nonce).
	pt := []byte("ATTACK AT DAWN")
	nonce := randBytes(12)
	frame, err := cli.Encrypt(ctx, nonce, nil, pt)
	if err != nil {
		fail("Encrypt for tamper: %v", err)
	}
	tampered := append([]byte{}, frame...)
	tampered[24] ^= 0x01 // first byte after header+nonce
	_, err = cli.Decrypt(ctx, tampered)
	if err == nil {
		fail("tampered ciphertext did NOT fail decrypt")
	}
	var ipcErr *ipc.ErrorResponse
	if !errors.As(err, &ipcErr) || ipcErr.Code != 401 {
		fail("tampered decrypt expected code 401, got: %v", err)
	}
	fmt.Printf("  ok tamper detected: %v\n", err)

	// Final status — counters check.
	st2, err := cli.GetStatus(ctx)
	if err != nil {
		fail("final status: %v", err)
	}
	fmt.Printf("AFTER : sealed=%d opened=%d errors=%d\n",
		st2.PacketsSealed, st2.PacketsOpened, st2.CryptoErrors)

	wantSealed := st.PacketsSealed + uint64(len(sizes)) + 1 // tamper test also sealed once
	wantOpened := st.PacketsOpened + uint64(len(sizes))     // tamper Decrypt did NOT increment opened (it errored)
	wantErrors := st.CryptoErrors + 1                       // exactly the tamper

	if st2.PacketsSealed != wantSealed {
		fail("packets_sealed: want %d, got %d", wantSealed, st2.PacketsSealed)
	}
	if st2.PacketsOpened != wantOpened {
		fail("packets_opened: want %d, got %d", wantOpened, st2.PacketsOpened)
	}
	if st2.CryptoErrors != wantErrors {
		fail("crypto_errors: want %d, got %d", wantErrors, st2.CryptoErrors)
	}

	fmt.Println("\nPASS")
}

func randBytes(n int) []byte {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return b
}

func fail(format string, args ...any) {
	fmt.Printf("\nFAIL: "+format+"\n", args...)
	os.Exit(1)
}
