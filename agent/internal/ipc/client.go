// Package ipc is the Go client for acm-cryptod's UDS control plane.
//
// Mirrors the message types defined in cryptod/crates/acm-ipc (Rust).
// Wire: line-delimited JSON over a Unix domain socket; one request per
// line, one response per line.
//
// Lock-step note: when the Rust enum tags / payload shapes change in
// crates/acm-ipc, update this file too. There is no proto/codegen for
// this control channel — JSON shape is the contract.
package ipc

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"sync"
	"time"
)

const DefaultSocketPath = "/run/acm/cryptod.sock"

// -- Request side --------------------------------------------------------

// Method names match the Rust `Request` enum's serde "snake_case" tags.
type request struct {
	Method string          `json:"method"`
	Params json.RawMessage `json:"params,omitempty"`
}

type rotateKeyParams struct {
	KeyID    uint32 `json:"key_id"`
	Algo     uint8  `json:"algo"`
	Material []byte `json:"material"`
}

// -- Response side -------------------------------------------------------

type responseEnvelope struct {
	Result string          `json:"result"`
	Data   json.RawMessage `json:"data,omitempty"`
}

// StatusReport mirrors acm_ipc::StatusReport in Rust.
type StatusReport struct {
	Version        string `json:"version"`
	Running        bool   `json:"running"`
	UptimeS        uint64 `json:"uptime_s"`
	ActiveProvider string `json:"active_provider"`
	ActiveKeyID    *uint32 `json:"active_key_id"`
	PacketsSealed  uint64 `json:"packets_sealed"`
	PacketsOpened  uint64 `json:"packets_opened"`
	CryptoErrors   uint64 `json:"crypto_errors"`
}

// ErrorResponse is what we hand to the caller for an Error envelope.
type ErrorResponse struct {
	Code    uint32 `json:"code"`
	Message string `json:"message"`
}

func (e *ErrorResponse) Error() string {
	return fmt.Sprintf("cryptod error %d: %s", e.Code, e.Message)
}

// -- Client --------------------------------------------------------------

// Client maintains a single, long-lived UDS connection to cryptod with
// request/response serialization. Concurrent callers are serialized via
// a mutex (the control plane is low-rate by design).
type Client struct {
	socket string
	mu     sync.Mutex
	conn   net.Conn
	reader *bufio.Reader
}

func New(socketPath string) *Client {
	if socketPath == "" {
		socketPath = DefaultSocketPath
	}
	return &Client{socket: socketPath}
}

func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.conn != nil {
		err := c.conn.Close()
		c.conn = nil
		c.reader = nil
		return err
	}
	return nil
}

func (c *Client) connect(ctx context.Context) error {
	if c.conn != nil {
		return nil
	}
	d := net.Dialer{}
	conn, err := d.DialContext(ctx, "unix", c.socket)
	if err != nil {
		return fmt.Errorf("dial %s: %w", c.socket, err)
	}
	c.conn = conn
	c.reader = bufio.NewReader(conn)
	return nil
}

// roundtrip sends one request line, reads one response line. Caller
// must already hold c.mu.
func (c *Client) roundtrip(ctx context.Context, req request) (responseEnvelope, error) {
	if err := c.connect(ctx); err != nil {
		return responseEnvelope{}, err
	}
	deadline, _ := ctx.Deadline()
	if deadline.IsZero() {
		deadline = time.Now().Add(10 * time.Second)
	}
	_ = c.conn.SetDeadline(deadline)

	payload, err := json.Marshal(req)
	if err != nil {
		return responseEnvelope{}, fmt.Errorf("marshal: %w", err)
	}
	payload = append(payload, '\n')
	if _, err := c.conn.Write(payload); err != nil {
		// Connection probably dead — drop it, caller may retry.
		_ = c.conn.Close()
		c.conn = nil
		c.reader = nil
		return responseEnvelope{}, fmt.Errorf("write: %w", err)
	}

	line, err := c.reader.ReadBytes('\n')
	if err != nil {
		_ = c.conn.Close()
		c.conn = nil
		c.reader = nil
		return responseEnvelope{}, fmt.Errorf("read: %w", err)
	}

	var env responseEnvelope
	if err := json.Unmarshal(line, &env); err != nil {
		return responseEnvelope{}, fmt.Errorf("parse response: %w (raw: %q)", err, line)
	}
	return env, nil
}

// GetStatus calls the cryptod GetStatus method.
func (c *Client) GetStatus(ctx context.Context) (*StatusReport, error) {
	c.mu.Lock()
	defer c.mu.Unlock()

	env, err := c.roundtrip(ctx, request{Method: "get_status"})
	if err != nil {
		return nil, err
	}
	switch env.Result {
	case "status":
		var s StatusReport
		if err := json.Unmarshal(env.Data, &s); err != nil {
			return nil, fmt.Errorf("decode status: %w", err)
		}
		return &s, nil
	case "error":
		var e ErrorResponse
		_ = json.Unmarshal(env.Data, &e)
		return nil, &e
	default:
		return nil, fmt.Errorf("unexpected response shape: %s", env.Result)
	}
}

// RotateKey calls the cryptod RotateKey method.
//
// algo is the AlgoId byte (0x01 = AES-128-GCM, 0x02 = AES-256-GCM,
// 0x10 = O'z DSt 1105). material length must match AlgoId.key_len().
func (c *Client) RotateKey(ctx context.Context, keyID uint32, algo uint8, material []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()

	params, _ := json.Marshal(rotateKeyParams{
		KeyID:    keyID,
		Algo:     algo,
		Material: material,
	})
	env, err := c.roundtrip(ctx, request{Method: "rotate_key", Params: params})
	if err != nil {
		return err
	}
	switch env.Result {
	case "ok":
		return nil
	case "error":
		var e ErrorResponse
		_ = json.Unmarshal(env.Data, &e)
		return &e
	default:
		return errors.New("unexpected response shape: " + env.Result)
	}
}
