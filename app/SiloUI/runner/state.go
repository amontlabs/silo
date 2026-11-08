package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"syscall"
	"time"
)

// Phase is where a computer the helper made is in its life. It is persisted, so a restart picks
// up exactly where the last run stopped.
type Phase string

const (
	// PhaseForking: the record exists and the fork was started (or is about to be); the computer
	// is known by name only.
	PhaseForking Phase = "forking"
	// PhaseLive: the computer exists and is known by id. Whether its runner is starting, idle or
	// busy is not persisted; it is derived again from GitHub after a restart.
	PhaseLive Phase = "live"
	// PhaseFinishing: the job ended, the start failed, the runner exited or sat idle. The runner is
	// deregistered at GitHub, then the computer is stopped (kept) or deleted.
	PhaseFinishing Phase = "finishing"
	// PhaseKept: the runner is deregistered and the computer is stopped. The only phase retention
	// may delete.
	PhaseKept Phase = "kept"
)

// Entry is a computer this helper made. It is recorded by name before the fork, gets the engine's
// computer id when the fork answers, and is removed once a fresh list no longer shows that id. The
// file lists exactly the computers the helper may still have to clean up; nothing else on the host
// is ever touched.
type Entry struct {
	// Name is chosen by the helper, ci-<label>-<id>, and never reused.
	Name string `json:"name"`
	// ID is the engine's computer id: empty until the fork has answered. Every engine call after
	// the fork goes by id, and a computer is ours only while its id is a record's id.
	ID    string `json:"id,omitempty"`
	Label string `json:"label"`
	Phase Phase  `json:"phase"`
	// Result is what ended the computer's runner: GitHub's job result, or the helper's own reason.
	Result string `json:"result,omitempty"`
	// At is when the record was made, and when a kept computer was kept.
	At time.Time `json:"at"`
	// Request is the job request the runner served, once known.
	Request int64 `json:"request,omitempty"`
}

// Ledger records the computers the helper made, in a file under the state directory. The event
// loop is its only writer, so it takes no lock.
type Ledger struct {
	path string
	data map[string]Entry
	// sync flushes a file to disk; replaced in tests.
	sync func(*os.File) error
}

// OpenLedger reads the ledger, or starts an empty one.
func OpenLedger(dir string) (*Ledger, error) {
	ledger := &Ledger{path: filepath.Join(dir, "computers.json"), data: map[string]Entry{}, sync: (*os.File).Sync}
	raw, err := os.ReadFile(ledger.path)
	switch {
	case errors.Is(err, os.ErrNotExist):
		return ledger, nil
	case err != nil:
		return nil, err
	}
	var entries []Entry
	if err := json.Unmarshal(raw, &entries); err != nil {
		return nil, fmt.Errorf("%s is unreadable: %w", ledger.path, err)
	}
	for _, entry := range entries {
		switch entry.Phase {
		case PhaseForking, PhaseLive, PhaseFinishing, PhaseKept:
		default:
			return nil, fmt.Errorf("%s is unreadable: %q has the unknown phase %q", ledger.path, entry.Name, entry.Phase)
		}
		ledger.data[entry.Name] = entry
	}
	return ledger, nil
}

// save writes the whole ledger: the file is flushed before it replaces the old one, and the
// directory is flushed after, so a crash leaves either the old ledger or the new one.
func (l *Ledger) save() error {
	raw, err := json.MarshalIndent(l.All(), "", "  ")
	if err != nil {
		return err
	}
	temp := l.path + ".tmp"
	file, err := os.OpenFile(temp, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return err
	}
	_, err = file.Write(raw)
	if err == nil {
		err = l.sync(file)
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	if err := os.Rename(temp, l.path); err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(l.path))
	if err != nil {
		return err
	}
	defer dir.Close()
	return l.sync(dir)
}

// Put records an entry, replacing the one of the same name.
func (l *Ledger) Put(entry Entry) error {
	l.data[entry.Name] = entry
	return l.save()
}

// Remove forgets a computer that is gone.
func (l *Ledger) Remove(name string) error {
	if _, ok := l.data[name]; !ok {
		return nil
	}
	delete(l.data, name)
	return l.save()
}

// Get returns the entry of a computer.
func (l *Ledger) Get(name string) (Entry, bool) {
	entry, ok := l.data[name]
	return entry, ok
}

// All returns every entry, ordered by name.
func (l *Ledger) All() []Entry {
	entries := make([]Entry, 0, len(l.data))
	for _, entry := range l.data {
		entries = append(entries, entry)
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name < entries[j].Name })
	return entries
}

// Lock is an exclusive lock on a file; one helper at a time owns a state directory.
type Lock struct{ file *os.File }

// AcquireLock takes the lock or fails when another helper holds it.
func AcquireLock(dir string) (*Lock, error) {
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, err
	}
	path := filepath.Join(dir, "lock")
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = file.Close()
		return nil, fmt.Errorf("another silo-runner is using %s (lock %s)", dir, path)
	}
	return &Lock{file: file}, nil
}

// Release gives the lock up.
func (l *Lock) Release() {
	if l != nil && l.file != nil {
		_ = syscall.Flock(int(l.file.Fd()), syscall.LOCK_UN)
		_ = l.file.Close()
	}
}
