package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/actions/scaleset"
)

// events is an ordered log shared by the fakes, so a test can see what happened first.
type events struct {
	mu   sync.Mutex
	list []string
}

func (e *events) add(format string, args ...any) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.list = append(e.list, fmt.Sprintf(format, args...))
}

func (e *events) all() []string {
	e.mu.Lock()
	defer e.mu.Unlock()
	return append([]string(nil), e.list...)
}

func (e *events) index(prefix string) int {
	for i, line := range e.all() {
		if strings.HasPrefix(line, prefix) {
			return i
		}
	}
	return -1
}

func (e *events) count(prefix string) int {
	n := 0
	for _, line := range e.all() {
		if strings.HasPrefix(line, prefix) {
			n++
		}
	}
	return n
}

// fakeProcess is a runner command. One that never prints the ready line ends at once, as a runner
// that fails to connect does; one that does stays up until it is killed.
type fakeProcess struct {
	lines chan string
	once  sync.Once
}

// holdOpen, as an output line, keeps a runner that is not ready alive.
const holdOpen = "<hold open>"

func newProcess(lines ...string) *fakeProcess {
	p := &fakeProcess{lines: make(chan string, 16)}
	ready, hold := false, false
	for _, line := range lines {
		if line == holdOpen {
			hold = true
			continue
		}
		p.lines <- line
		ready = ready || strings.Contains(line, readyLine)
	}
	if !ready && !hold {
		p.Kill()
	}
	return p
}

func (p *fakeProcess) Lines() <-chan string { return p.lines }
func (p *fakeProcess) Wait() error          { <-p.lines; return nil }
func (p *fakeProcess) Kill()                { p.once.Do(func() { close(p.lines) }) }

// fakeSilo keeps computers in memory, by id as the engine does.
type fakeSilo struct {
	log *events
	mu  sync.Mutex
	// computers by id.
	computers map[string]Computer
	procs     map[string]*fakeProcess
	// output decides what the runner prints for the nth runner started (from 0).
	output  func(n int) []string
	started int
	// inputs by computer id.
	inputs   map[string]string
	argvs    [][]string
	failFork bool
	// forkErr, when set, is the error Fork answers with.
	forkErr error
	// forkHook runs inside Fork, before it answers; forkGate, when set, holds Fork until closed.
	forkHook func(name string)
	forkGate chan struct{}
	built    []string
	buildErr error
	// buildHook runs inside BuildRecipe.
	buildHook func()
	listErr   error
	nextID    int
	// dropErr and stopErr, when set, make Drop and Stop fail; onRunner runs while a runner is being
	// started.
	dropErr  error
	stopErr  error
	onRunner func(id string)
	// staleList, when set, is what List answers: a cached inventory.
	staleList []Computer
}

func newSilo(log *events) *fakeSilo {
	return &fakeSilo{log: log, computers: map[string]Computer{}, inputs: map[string]string{}, procs: map[string]*fakeProcess{},
		output: func(int) []string { return []string{"Connected to GitHub", "Listening for Jobs"} }}
}

// add puts a computer in; its id defaults to its name.
func (f *fakeSilo) add(c Computer) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if c.ID == "" {
		c.ID = c.Name
	}
	f.computers[c.ID] = c
}

func (f *fakeSilo) find(name string) (Computer, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, c := range f.computers {
		if c.Name == name {
			return c, true
		}
	}
	return Computer{}, false
}

func (f *fakeSilo) names() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	var names []string
	for _, c := range f.computers {
		names = append(names, c.Name)
	}
	return names
}

func (f *fakeSilo) state(name string) (string, bool) {
	c, ok := f.find(name)
	return c.State, ok
}

func (f *fakeSilo) idOf(name string) string {
	c, _ := f.find(name)
	return c.ID
}

func (f *fakeSilo) setState(name, state string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for id, c := range f.computers {
		if c.Name == name {
			c.State = state
			f.computers[id] = c
		}
	}
}

// label names a computer by id in the log, which is how a test reads it.
func (f *fakeSilo) label(id string) string {
	f.mu.Lock()
	defer f.mu.Unlock()
	if c, ok := f.computers[id]; ok {
		return c.Name
	}
	return "?" + id
}

func (f *fakeSilo) List(context.Context) ([]Computer, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.listErr != nil {
		return nil, f.listErr
	}
	if f.staleList != nil {
		return append([]Computer(nil), f.staleList...), nil
	}
	var out []Computer
	for _, c := range f.computers {
		out = append(out, c)
	}
	return out, nil
}

func (f *fakeSilo) Fork(_ context.Context, name, recipe string) (string, error) {
	f.log.add("fork %s %s", name, recipe)
	f.mu.Lock()
	gate, hook := f.forkGate, f.forkHook
	f.mu.Unlock()
	if hook != nil {
		hook(name)
	}
	if gate != nil {
		<-gate
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.forkErr != nil {
		return "", f.forkErr
	}
	if f.failFork {
		return "", errors.New("fork failed")
	}
	kind := "linux"
	if strings.Contains(recipe, "mac") {
		kind = "macos"
	}
	f.nextID++
	id := fmt.Sprintf("id-%d", f.nextID)
	f.computers[id] = Computer{ID: id, Name: name, Kind: kind, State: "stopped", MemoryGiB: 8}
	return id, nil
}

// act changes the state of the computer with the id, or fails when there is none.
func (f *fakeSilo) act(verb, id, state string) error {
	f.log.add("%s %s", verb, f.label(id))
	f.mu.Lock()
	defer f.mu.Unlock()
	c, ok := f.computers[id]
	if !ok {
		return errors.New("no computer has the id " + id)
	}
	c.State = state
	f.computers[id] = c
	return nil
}

func (f *fakeSilo) Start(_ context.Context, id string) error { return f.act("start", id, "running") }
func (f *fakeSilo) Stop(_ context.Context, id string) error {
	f.mu.Lock()
	err := f.stopErr
	f.mu.Unlock()
	if err != nil {
		f.log.add("stop %s", f.label(id))
		return err
	}
	return f.act("stop", id, "stopped")
}

func (f *fakeSilo) Drop(_ context.Context, id string) error {
	f.log.add("drop %s", f.label(id))
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.dropErr != nil {
		return f.dropErr
	}
	if _, ok := f.computers[id]; !ok {
		return errors.New("no computer has the id " + id)
	}
	delete(f.computers, id)
	return nil
}

func (f *fakeSilo) StartRunner(_ context.Context, id, input string, argv []string) (Process, error) {
	f.log.add("runner %s", f.label(id))
	f.mu.Lock()
	n := f.started
	f.started++
	f.inputs[id] = input
	f.argvs = append(f.argvs, append([]string{id}, argv...))
	hook := f.onRunner
	f.mu.Unlock()
	if hook != nil {
		hook(id)
	}
	proc := newProcess(f.output(n)...)
	f.mu.Lock()
	f.procs[id] = proc
	f.mu.Unlock()
	return proc, nil
}

func (f *fakeSilo) BuildRecipe(_ context.Context, recipe string) error {
	f.log.add("build %s", recipe)
	f.mu.Lock()
	hook := f.buildHook
	f.mu.Unlock()
	if hook != nil {
		hook()
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.built = append(f.built, recipe)
	return f.buildErr
}

// fakeActions stands in for GitHub's scale set API.
type fakeActions struct {
	log *events
	mu  sync.Mutex
	// next runner id.
	next int
	// runners by name.
	runners map[string]int
	// removeErr by runner id.
	removeErr map[int64]error
	jitErr    error
	// lookupErr makes GetRunnerByName fail.
	lookupErr error
}

func newActions(log *events) *fakeActions {
	return &fakeActions{log: log, next: 100, runners: map[string]int{}, removeErr: map[int64]error{}}
}

func (a *fakeActions) GenerateJitRunnerConfig(_ context.Context, setting *scaleset.RunnerScaleSetJitRunnerSetting, _ int) (*scaleset.RunnerScaleSetJitRunnerConfig, error) {
	a.log.add("jit %s", setting.Name)
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.jitErr != nil {
		return nil, a.jitErr
	}
	a.next++
	a.runners[setting.Name] = a.next
	return &scaleset.RunnerScaleSetJitRunnerConfig{
		Runner:           &scaleset.RunnerReference{ID: a.next, Name: setting.Name},
		EncodedJITConfig: "JIT-SECRET-" + setting.Name,
	}, nil
}

func (a *fakeActions) GetRunnerByName(_ context.Context, name string) (*scaleset.RunnerReference, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.lookupErr != nil {
		return nil, a.lookupErr
	}
	id, ok := a.runners[name]
	if !ok {
		return nil, nil
	}
	return &scaleset.RunnerReference{ID: id, Name: name}, nil
}

func (a *fakeActions) RemoveRunner(_ context.Context, id int64) error {
	a.log.add("deregister %d", id)
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.removeErr[id]; err != nil {
		return err
	}
	for name, known := range a.runners {
		if int64(known) == id {
			delete(a.runners, name)
		}
	}
	return nil
}

func (a *fakeActions) idOf(name string) int64 {
	a.mu.Lock()
	defer a.mu.Unlock()
	return int64(a.runners[name])
}

func (a *fakeActions) forget(name string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	delete(a.runners, name)
}

type fakeAcquirer struct{ calls [][]int64 }

func (f *fakeAcquirer) AcquireJobs(_ context.Context, ids []int64) ([]int64, error) {
	f.calls = append(f.calls, ids)
	return ids, nil
}

// harness wires a Fleet to the fakes.
type harness struct {
	t          *testing.T
	log        *events
	silo       *fakeSilo
	actions    *fakeActions
	ledger     *Ledger
	fleet      *Fleet
	scaler     *Scaler
	opts       harnessOptions
	stateDir   string
	projectDir string
	now        time.Time
	nowMu      sync.Mutex
	ids        int
}

type harnessOptions struct {
	spec     ScaleSetSpec
	freeDisk int
	linux    LinuxBudget
	// recipeMemory is the memory the recipe file gives; 8 when zero.
	recipeMemory int
	// attempts is startAttempts; 3 when zero.
	attempts int
	// readyTimeout is how long a runner may take to listen; 200ms when zero.
	readyTimeout time.Duration
}

func newHarness(t *testing.T, o harnessOptions) *harness {
	t.Helper()
	if o.spec.Label == "" {
		o.spec = ScaleSetSpec{Label: "silo-linux", Recipe: "silo-linux", OS: "linux", MaxRunners: 3, KeepFailed: KeepFailed{Count: intp(2), Days: 7}}
	}
	if o.freeDisk == 0 {
		o.freeDisk = 500
	}
	if o.recipeMemory == 0 {
		o.recipeMemory = 8
	}
	if o.attempts == 0 {
		o.attempts = 3
	}
	if o.readyTimeout == 0 {
		o.readyTimeout = 200 * time.Millisecond
	}
	log := &events{}
	h := &harness{t: t, log: log, silo: newSilo(log), actions: newActions(log), opts: o,
		stateDir: t.TempDir(), projectDir: t.TempDir(), now: time.Date(2026, 11, 8, 12, 0, 0, 0, time.UTC)}
	h.writeRecipe(o.spec.Recipe, o.recipeMemory)
	h.start()
	t.Cleanup(func() { h.fleet.Shutdown() })
	return h
}

func (h *harness) writeRecipe(name string, memory int) {
	h.t.Helper()
	dir := filepath.Join(h.projectDir, ".silo", "recipes")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		h.t.Fatal(err)
	}
	text := fmt.Sprintf("os: %s\nmemory: %d\n", h.opts.spec.OS, memory)
	if err := os.WriteFile(filepath.Join(dir, name+".yaml"), []byte(text), 0o644); err != nil {
		h.t.Fatal(err)
	}
}

// start opens the ledger from disk and starts a fleet on it.
func (h *harness) start() {
	h.t.Helper()
	ledger, err := OpenLedger(h.stateDir)
	if err != nil {
		h.t.Fatal(err)
	}
	h.ledger = ledger
	h.fleet = NewFleet(FleetOptions{
		Sets:         []FleetSet{{Spec: h.opts.spec, ScaleSetID: 7, Start: []string{"/opt/actions-runner/silo-run-jit"}}},
		ProjectDir:   h.projectDir,
		Linux:        h.opts.linux,
		ReadyTimeout: h.opts.readyTimeout, StartAttempts: h.opts.attempts, IdleTimeout: time.Minute,
		Logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
		Now: func() time.Time {
			h.nowMu.Lock()
			defer h.nowMu.Unlock()
			return h.now
		},
		NewID: func() string {
			h.nowMu.Lock()
			defer h.nowMu.Unlock()
			h.ids++
			return fmt.Sprintf("%08d", h.ids)
		},
	}, h.actions, h.silo, DiskGuard{Path: "/", MinGiB: 20, Free: func(string) (int, error) { return h.opts.freeDisk, nil }}, h.ledger, nil)
	h.scaler = h.fleet.Scaler(h.opts.spec.Label)
	h.scaler.SetAcquirer(&fakeAcquirer{})
	h.fleet.Start()
	h.fleet.Settle()
}

// restart stops the helper and starts it again on the ledger it left, with entries added: the
// records a run that crashed would have left.
func (h *harness) restart(entries ...Entry) {
	h.t.Helper()
	h.fleet.Shutdown()
	ledger, err := OpenLedger(h.stateDir)
	if err != nil {
		h.t.Fatal(err)
	}
	for _, entry := range entries {
		if err := ledger.Put(entry); err != nil {
			h.t.Fatal(err)
		}
	}
	h.start()
}

func (h *harness) advance(d time.Duration) {
	h.nowMu.Lock()
	defer h.nowMu.Unlock()
	h.now = h.now.Add(d)
}

func (h *harness) clock() time.Time {
	h.nowMu.Lock()
	defer h.nowMu.Unlock()
	return h.now
}

// scale delivers a message and waits until the fleet has nothing left to do.
func (h *harness) scale(msg *scaleset.RunnerScaleSetMessage) {
	h.t.Helper()
	if err := h.scaler.Scale(context.Background(), msg); err != nil {
		h.t.Fatal(err)
	}
	h.fleet.Settle()
}

// tick runs the periodic plan and waits.
func (h *harness) tick() {
	h.t.Helper()
	h.fleet.Tick()
	h.fleet.Settle()
}

// later moves the clock on and ticks.
func (h *harness) later(d time.Duration) {
	h.t.Helper()
	h.advance(d)
	h.tick()
}

// exited tells the fleet that the runner command of a computer ended.
func (h *harness) exited(name string) {
	h.t.Helper()
	h.fleet.Deliver(runnerExited{name: name})
	h.fleet.Settle()
}

func (h *harness) status() map[string]string { return h.fleet.Runners() }

func available(ids ...int64) *scaleset.RunnerScaleSetMessage {
	msg := &scaleset.RunnerScaleSetMessage{Statistics: &scaleset.RunnerScaleSetStatistic{TotalAssignedJobs: len(ids)}}
	for _, id := range ids {
		msg.JobAvailableMessages = append(msg.JobAvailableMessages, &scaleset.JobAvailable{JobMessageBase: scaleset.JobMessageBase{RunnerRequestID: id}})
	}
	return msg
}

func started(request int64, runner string) *scaleset.RunnerScaleSetMessage {
	return &scaleset.RunnerScaleSetMessage{
		Statistics:         &scaleset.RunnerScaleSetStatistic{TotalAssignedJobs: 1},
		JobStartedMessages: []*scaleset.JobStarted{{RunnerName: runner, JobMessageBase: scaleset.JobMessageBase{RunnerRequestID: request}}},
	}
}

func completed(request int64, runner, result string, assigned int) *scaleset.RunnerScaleSetMessage {
	return &scaleset.RunnerScaleSetMessage{
		Statistics:           &scaleset.RunnerScaleSetStatistic{TotalAssignedJobs: assigned},
		JobCompletedMessages: []*scaleset.JobCompleted{{Result: result, RunnerName: runner, JobMessageBase: scaleset.JobMessageBase{RunnerRequestID: request}}},
	}
}

// onlyRunner returns the name of the single record the fleet has.
func (h *harness) onlyRunner() string {
	h.t.Helper()
	runners := h.status()
	if len(runners) != 1 {
		h.t.Fatalf("want one record, have %v", runners)
	}
	for name := range runners {
		return name
	}
	return ""
}

func intp(n int) *int { return &n }

// only returns the name of the single record in the state ("idle", "kept", ...).
func (h *harness) only(state string) string {
	h.t.Helper()
	var found []string
	for name, got := range h.status() {
		if got == state {
			found = append(found, name)
		}
	}
	if len(found) != 1 {
		h.t.Fatalf("want one %s record, have %v", state, h.status())
	}
	return found[0]
}

// waitFor polls until cond holds; it is for the few things that happen on their own goroutine.
func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(2 * time.Millisecond)
	}
}
