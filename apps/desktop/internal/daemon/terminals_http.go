package daemon

import (
	"encoding/json"
	"net/http"
)

func (d *Daemon) handleListTerminals(w http.ResponseWriter, r *http.Request) {
	if _, err := d.store.GetSession(r.PathValue("id")); err != nil {
		writeStoreError(w, err)
		return
	}
	terminals, err := d.store.ListTerminals(r.PathValue("id"))
	if err != nil {
		writeError(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"terminals": terminals})
}

func (d *Daemon) handleCreateTerminal(w http.ResponseWriter, r *http.Request) {
	session, err := d.store.GetSession(r.PathValue("id"))
	if err != nil {
		writeStoreError(w, err)
		return
	}
	var body struct {
		Title string `json:"title"`
	}
	if r.ContentLength > 0 {
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			writeError(w, http.StatusBadRequest, err)
			return
		}
	}
	terminal, err := d.terminals.Create(session, body.Title)
	if err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	writeJSON(w, http.StatusCreated, terminal)
}

func (d *Daemon) handleTerminalStream(w http.ResponseWriter, r *http.Request) {
	initial, output, cancel, err := d.terminals.Subscribe(r.PathValue("id"))
	if err != nil {
		writeError(w, http.StatusNotFound, err)
		return
	}
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		cancel()
		return
	}
	defer conn.Close()
	defer cancel()
	if len(initial) > 0 {
		_ = conn.WriteJSON(map[string]any{"type": "output", "data": string(initial), "replay": true})
	}
	for chunk := range output {
		if err := conn.WriteJSON(map[string]any{"type": "output", "data": string(chunk)}); err != nil {
			return
		}
	}
	terminal, _ := d.store.GetTerminal(r.PathValue("id"))
	_ = conn.WriteJSON(map[string]any{"type": "status", "status": terminal.Status, "exit_code": terminal.ExitCode})
}

func (d *Daemon) handleTerminalInput(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Data string `json:"data"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	if err := d.terminals.Write(r.PathValue("id"), body.Data); err != nil {
		writeError(w, http.StatusConflict, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (d *Daemon) handleTerminalResize(w http.ResponseWriter, r *http.Request) {
	var body struct{ Rows, Cols uint16 }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	if err := d.terminals.Resize(r.PathValue("id"), body.Rows, body.Cols); err != nil {
		writeError(w, http.StatusConflict, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (d *Daemon) handleTerminalStop(w http.ResponseWriter, r *http.Request) {
	if err := d.terminals.Stop(r.PathValue("id")); err != nil {
		writeError(w, http.StatusConflict, err)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
