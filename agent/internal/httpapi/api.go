// Package httpapi exposes a small REST surface for managing cryptod
// from a browser or curl.
//
// Endpoints:
//
//	GET  /api/v1/status              StatusReport as JSON
//	POST /api/v1/keys/rotate         {key_id, algo, material_hex} -> {ok|error}
//	GET  /healthz                    plain 200 if agent is alive
//	GET  /readyz                     200 if cryptod responded recently
//
// All write operations are JSON in / JSON out. Authentication is NOT in
// scope for this iteration — agent listens on loopback / management VLAN
// only. mTLS / OIDC will be wired in when controller integration starts.
package httpapi

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"time"

	"github.com/zolotaevdmitriy-droid/smartsfp/agent/internal/ipc"
)

type API struct {
	cli *ipc.Client
}

func New(cli *ipc.Client) *API {
	return &API{cli: cli}
}

// Routes attaches all endpoints to the given mux.
func (a *API) Routes(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/status", a.handleStatus)
	mux.HandleFunc("/api/v1/keys/rotate", a.handleRotateKey)
	mux.HandleFunc("/healthz", a.handleHealthz)
	mux.HandleFunc("/readyz", a.handleReadyz)
}

func (a *API) handleStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)
	defer cancel()
	st, err := a.cli.GetStatus(ctx)
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, st)
}

type rotateKeyRequest struct {
	KeyID       uint32 `json:"key_id"`
	Algo        uint8  `json:"algo"`
	MaterialHex string `json:"material_hex"`
}

func (a *API) handleRotateKey(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req rotateKeyRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}
	material, err := hex.DecodeString(req.MaterialHex)
	if err != nil {
		writeError(w, http.StatusBadRequest, "material_hex: "+err.Error())
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()
	if err := a.cli.RotateKey(ctx, req.KeyID, req.Algo, material); err != nil {
		// If cryptod returned a typed error, mirror its code into HTTP.
		var ipcErr *ipc.ErrorResponse
		if errors.As(err, &ipcErr) {
			status := http.StatusInternalServerError
			switch {
			case ipcErr.Code >= 400 && ipcErr.Code < 500:
				status = int(ipcErr.Code)
			case ipcErr.Code == 501:
				status = http.StatusNotImplemented
			}
			writeError(w, status, ipcErr.Message)
			return
		}
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"result": "ok"})
}

func (a *API) handleHealthz(w http.ResponseWriter, r *http.Request) {
	_, _ = w.Write([]byte("ok\n"))
}

func (a *API) handleReadyz(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 1*time.Second)
	defer cancel()
	if _, err := a.cli.GetStatus(ctx); err != nil {
		writeError(w, http.StatusServiceUnavailable, "cryptod unreachable: "+err.Error())
		return
	}
	_, _ = w.Write([]byte("ready\n"))
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, code int, msg string) {
	writeJSON(w, code, map[string]string{"error": msg})
}
