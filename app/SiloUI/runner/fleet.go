package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"sync"
	"time"

	"github.com/actions/scaleset"
	"github.com/google/uuid"
)

// The Fleet is the helper's whole lifecycle in one place. A single goroutine, the event loop,
// receives every event (a scale set message, a runner command ending, a finished task, a timer
// tick, shutdown) and is the only writer of the records and of the ledger. Slow calls (fork,
// start, deregister, stop, drop, list) run as tasks in their own goroutines and come back as
// events. After every event plan() looks at every record and at the latest list, and starts the
// next step of whatever has no task in flight. Nothing else decides anything, so a step that fails
// is simply started again by a later plan().

const (
	// retryAfter is how long a record waits after a failed step. A tick retries it.
	retryAfter = 30 * time.Second
	// exitGrace is how long a busy runner whose command ended is given to receive its
	// JobCompleted message before GitHub is asked whether the runner still exists.
	exitGrace = 2 * time.Minute
	// probeEvery is how often a busy runner that has no command to watch is looked up at GitHub.
	probeEvery = 30 * time.Second
)

// sub is where a live runner is. It is memory only. After a restart a live record is busy until
// GitHub says otherwise: its runner may have a job.
type sub int

const (
	subStarting sub = iota
	subIdle
	subBusy
)

// step is how far a finishing record has come: deregister the runner, then stop or delete the
// computer, then wait for a fresh list to show the computer gone.
type step int

const (
	stepDeregister step = iota
	stepDispose
	stepVerify
)

type taskKind int

const (
	taskNone taskKind = iota
	taskFork
	taskUp
	taskDeregister
	taskStop
	taskDrop
	taskProbe
)

// record is a ledger entry plus what is only worth keeping in memory.
type record struct {
	Entry
	sub   sub
	since time.Time
	// proc is the command running GitHub's runner in the computer, nil after a restart or once it ended.
	proc Process
	task taskKind
	// notBefore holds the next step back after a failure.
	notBefore time.Time
	step      step
	// seenFrom is the first list that can show this computer: lists started before its fork
	// answered (or before its drop) say nothing about it.
	seenFrom int
}

// setState is the memory of one scale set.
type setState struct {
	spec  ScaleSetSpec
	id    int
	start []string

	outstanding   map[int64]struct{}
	finished      map[int64]struct{}
	finishedOrder []int64
	assigned      int
	attempts      int
	failStreak    int
	backoffTill   time.Time
	scaler        *Scaler
}

// FleetOptions are the knobs of a Fleet.
type FleetOptions struct {
	Sets          []FleetSet
	ProjectDir    string
	Linux         LinuxBudget
	ReadyTimeout  time.Duration
	StartAttempts int
	IdleTimeout   time.Duration
	// TickEvery is how often the loop plans on its own. Zero means never (tests send ticks).
	TickEvery time.Duration
	Logger    *slog.Logger
	// CheckJobAccess runs the private-repository check again before a job is taken; nil checks
	// nothing (tests).
	CheckJobAccess func(context.Context) error
	// Now and NewID are replaced in tests.
	Now   func() time.Time
	NewID func() string
}

// FleetSet is a scale set the fleet serves.
type FleetSet struct {
	Spec       ScaleSetSpec
	ScaleSetID int
	// Start is the command that starts GitHub's runner inside a computer.
	Start []string
}

// Fleet owns every computer the helper makes.
type Fleet struct {
	opts   FleetOptions
	gh     Actions
	silo   Silo
	disk   DiskGuard
	ledger *Ledger
	// gates are held for reading while a computer is forked from a recipe and for writing while the
	// recipe is rebuilt.
	gates map[string]*sync.RWMutex

	events chan any
	done   chan struct{}
	ctx    context.Context
	cancel context.CancelFunc
	// upCtx bounds the tasks that bring a runner up; shutdown cancels it.
	upCtx    context.Context
	upCancel context.CancelFunc

	// Everything below belongs to the event loop.
	sets     map[string]*setState
	records  map[string]*record
	inflight int
	closing  bool
	// list is the latest successful `silo list --all`; listStarts counts the lists started, and
	// listBusy says one is in flight. fresh is true only while the plan() right after a list
	// arrived: a list is used for one round of decisions and then asked for again.
	list []Computer
	// listSeq is the number of the list f.list came from.
	listSeq    int
	listStarts int
	listBusy   bool
	// listNotBefore holds the next list back after one failed.
	listNotBefore time.Time
	fresh         bool
	settlers      []chan struct{}
	replies       []chan struct{}
	// rebuilds are the recipe rebuilds in progress: each runs a computer the list may not show yet.
	rebuilds   map[int]Demand
	rebuildSeq int
}

// NewFleet makes a Fleet from the ledger's records. It does nothing until Start.
func NewFleet(opts FleetOptions, gh Actions, silo Silo, disk DiskGuard, ledger *Ledger, gates map[string]*sync.RWMutex) *Fleet {
	if opts.Now == nil {
		opts.Now = time.Now
	}
	if opts.NewID == nil {
		opts.NewID = func() string { return uuid.NewString()[:8] }
	}
	if opts.Logger == nil {
		opts.Logger = slog.New(slog.DiscardHandler)
	}
	if gates == nil {
		gates = map[string]*sync.RWMutex{}
	}
	f := &Fleet{
		opts: opts, gh: gh, silo: silo, disk: disk, ledger: ledger, gates: gates,
		events: make(chan any, 256), done: make(chan struct{}),
		sets: map[string]*setState{}, records: map[string]*record{}, rebuilds: map[int]Demand{},
	}
	f.ctx, f.cancel = context.WithCancel(context.Background())
	f.upCtx, f.upCancel = context.WithCancel(f.ctx)
	for _, set := range opts.Sets {
		state := &setState{spec: set.Spec, id: set.ScaleSetID, start: set.Start, outstanding: map[int64]struct{}{}, finished: map[int64]struct{}{}}
		state.scaler = &Scaler{fleet: f, label: set.Spec.Label, check: opts.CheckJobAccess}
		state.scaler.beat.Store(opts.Now().UnixNano())
		f.sets[set.Spec.Label] = state
		if f.gates[set.Spec.Recipe] == nil {
			f.gates[set.Spec.Recipe] = &sync.RWMutex{}
		}
	}
	for _, entry := range ledger.All() {
		rec := &record{Entry: entry, since: opts.Now(), sub: subBusy}
		if entry.Phase == PhaseKept {
			rec.step = stepDispose
		}
		f.records[entry.Name] = rec
		switch {
		case f.sets[entry.Label] == nil:
			opts.Logger.Warn("a computer belongs to a label that is no longer configured; leaving it", "computer", entry.Name, "label", entry.Label)
		case entry.Phase == PhaseLive && entry.Request == 0 && entry.Result == "":
			// Nothing says the runner ever had a job: deregistering it finds out, because GitHub
			// refuses while it has one and the computer is adopted as busy again.
			f.finish(rec, resultOrphan)
		}
	}
	return f
}

// Scaler returns the listener's view of the scale set with the label.
func (f *Fleet) Scaler(label string) *Scaler { return f.sets[label].scaler }

// Start runs the event loop, with a first plan right away: a restart is not a special case.
func (f *Fleet) Start() {
	go f.loop()
	f.post(tickEvent{})
	if f.opts.TickEvery > 0 {
		go func() {
			ticker := time.NewTicker(f.opts.TickEvery)
			defer ticker.Stop()
			for {
				select {
				case <-ticker.C:
					f.post(tickEvent{})
				case <-f.done:
					return
				}
			}
		}()
	}
}

// Shutdown deregisters and deletes the idle runners and waits for the work in flight; a runner
// with a job keeps going and is picked up by the next start.
func (f *Fleet) Shutdown() {
	f.post(shutdownEvent{})
	<-f.done
}

// Tick plans once, as the timer does.
func (f *Fleet) Tick() { f.post(tickEvent{}) }

// Settle blocks until no task is in flight and no event waits: the fleet has nothing left to do
// until something new happens. Tests use it instead of sleeping.
func (f *Fleet) Settle() {
	reply := make(chan struct{})
	f.post(settleEvent{reply})
	select {
	case <-reply:
	case <-f.done:
	}
}

// Deliver hands the loop an event from outside, as a runner command ending would.
func (f *Fleet) Deliver(ev any) { f.post(ev) }

var errFleetStopped = errors.New("the fleet has stopped")

func (f *Fleet) post(ev any) {
	select {
	case f.events <- ev:
	case <-f.done:
	}
}

func (f *Fleet) deliver(ev messageEvent) error {
	select {
	case f.events <- ev:
	case <-f.done:
		return errFleetStopped
	}
	select {
	case <-ev.reply:
		return nil
	case <-f.done:
		return errFleetStopped
	}
}

// query runs fn on the event loop and waits for it.
func (f *Fleet) query(fn func()) {
	done := make(chan struct{})
	f.post(queryEvent{fn: fn, done: done})
	select {
	case <-done:
	case <-f.done:
	}
}

// Runners returns the state of every record by name, for tests and status.
func (f *Fleet) Runners() map[string]string {
	out := map[string]string{}
	f.query(func() {
		for name, rec := range f.records {
			out[name] = rec.status()
		}
	})
	return out
}

// idle says the record's runner is up and waiting for a job.
func (r *record) idle() bool { return r.Phase == PhaseLive && r.sub == subIdle && r.proc != nil }

// idleFor says it has been so, with nothing else going on, for at least timeout.
func (r *record) idleFor(now time.Time, timeout time.Duration) bool {
	return r.idle() && r.task == taskNone && now.Sub(r.since) >= timeout
}

func (r *record) status() string {
	if r.Phase == PhaseLive {
		return [...]string{"starting", "idle", "busy"}[r.sub]
	}
	return string(r.Phase)
}

// Reserve asks whether one more computer of the operating system and memory fits, from a fresh
// list and the computers the fleet has asked for that the list does not show running yet. When it
// does, the computer counts against every other decision until release is called: a recipe
// rebuild holds one while it runs. While the list shows the rebuild's own virtual machine too, it
// is counted twice, which is the safe direction.
func (f *Fleet) Reserve(ctx context.Context, osName string, memoryGiB int) (release func(), err error) {
	computers, err := f.silo.List(ctx)
	if err != nil {
		return nil, fmt.Errorf("cannot read the computers to check capacity: %w", err)
	}
	var token int
	f.query(func() {
		err = Capacity{Linux: f.opts.Linux}.Allows(osName, memoryGiB, computers, f.pendingDemands(computers, -1))
		if err == nil {
			f.rebuildSeq++
			token = f.rebuildSeq
			f.rebuilds[token] = Demand{OS: osName, MemoryGiB: memoryGiB}
		}
	})
	if err != nil {
		return nil, err
	}
	return func() { f.query(func() { delete(f.rebuilds, token) }) }, nil
}

// MARK: events

type (
	messageEvent struct {
		label     string
		acquired  []int64
		started   []*scaleset.JobStarted
		completed []*scaleset.JobCompleted
		assigned  *int
		reply     chan struct{}
	}
	tickEvent     struct{}
	shutdownEvent struct{}
	settleEvent   struct{ reply chan struct{} }
	queryEvent    struct {
		fn   func()
		done chan struct{}
	}
	// runnerExited: the command that ran GitHub's runner ended.
	runnerExited struct {
		name string
		proc Process
	}
	forkDone struct {
		name, id string
		err      error
		// nameTaken: the engine refused the name, so no computer was made.
		nameTaken bool
	}
	upDone struct {
		name string
		proc Process
		err  error
	}
	deregisterDone struct {
		name       string
		jobRunning bool
		err        error
	}
	stopDone struct {
		name string
		err  error
	}
	dropDone struct {
		name string
		err  error
	}
	probeDone struct {
		name    string
		present bool
		err     error
	}
	listDone struct {
		seq       int
		computers []Computer
		err       error
	}
)

func (f *Fleet) loop() {
	defer func() {
		f.cancel()
		close(f.done)
	}()
	for {
		ev := <-f.events
		tick := f.apply(ev)
		f.plan(tick)
		f.fresh = false
		closeAll(&f.replies)
		if f.inflight == 0 && (f.closing || len(f.events) == 0) {
			closeAll(&f.settlers)
		}
		if f.closing && f.inflight == 0 {
			return
		}
	}
}

func closeAll(waiting *[]chan struct{}) {
	for _, reply := range *waiting {
		close(reply)
	}
	*waiting = nil
}

// apply makes the change an event stands for. It reports whether the event is a tick.
func (f *Fleet) apply(ev any) (tick bool) {
	now := f.opts.Now()
	switch ev := ev.(type) {
	case tickEvent:
		return true
	case shutdownEvent:
		f.closing = true
		f.upCancel()
	case settleEvent:
		f.settlers = append(f.settlers, ev.reply)
	case queryEvent:
		ev.fn()
		close(ev.done)
	case messageEvent:
		f.onMessage(ev, now)
		f.replies = append(f.replies, ev.reply)
	case runnerExited:
		f.onExited(ev, now)
	case listDone:
		f.inflight--
		f.listBusy = false
		f.onListed(ev, now)
	case forkDone:
		rec := f.taskDone(ev.name)
		f.onForkDone(rec, ev, now)
	case upDone:
		f.onUpDone(f.taskDone(ev.name), ev, now)
	case deregisterDone:
		f.onDeregisterDone(f.taskDone(ev.name), ev, now)
	case stopDone:
		f.onStopDone(f.taskDone(ev.name), ev, now)
	case dropDone:
		f.onDropDone(f.taskDone(ev.name), ev, now)
	case probeDone:
		f.onProbeDone(f.taskDone(ev.name), ev, now)
	}
	return false
}

// taskDone ends the task a result event belongs to.
func (f *Fleet) taskDone(name string) *record {
	f.inflight--
	rec := f.records[name]
	if rec != nil {
		rec.task = taskNone
	}
	return rec
}

func (f *Fleet) persist(rec *record) {
	if err := f.ledger.Put(rec.Entry); err != nil {
		f.opts.Logger.Error("cannot write the computer record", "computer", rec.Name, "error", err.Error())
	}
}

func (f *Fleet) forget(rec *record) {
	if err := f.ledger.Remove(rec.Name); err != nil {
		f.opts.Logger.Error("cannot remove the computer record", "computer", rec.Name, "error", err.Error())
	}
	delete(f.records, rec.Name)
}

// finish moves a live record to finishing with the reason its runner ended.
func (f *Fleet) finish(rec *record, result string) {
	rec.Phase, rec.Result, rec.step, rec.notBefore = PhaseFinishing, result, stepDeregister, time.Time{}
	f.persist(rec)
	if set := f.sets[rec.Label]; set != nil {
		// The runner's job request is over with it; do not ask for a replacement runner for it.
		set.retire(rec.Request)
	}
}

func (f *Fleet) onMessage(m messageEvent, now time.Time) {
	set := f.sets[m.label]
	for _, id := range m.acquired {
		if _, done := set.finished[id]; !done {
			set.outstanding[id] = struct{}{}
		}
	}
	for _, started := range m.started {
		f.opts.Logger.Info("job started", "runner", started.RunnerName, "job", started.JobDisplayName, "request", started.RunnerRequestID)
		rec := f.records[started.RunnerName]
		if rec == nil || rec.Phase == PhaseKept || rec.Phase == PhaseForking {
			continue
		}
		rec.Request = started.RunnerRequestID
		if rec.Phase == PhaseLive {
			rec.sub, rec.since = subBusy, now
		}
		f.persist(rec)
	}
	for _, completed := range m.completed {
		set.retire(completed.RunnerRequestID)
		f.opts.Logger.Info("job completed", "runner", completed.RunnerName, "result", completed.Result, "request", completed.RunnerRequestID)
		rec := f.records[completed.RunnerName]
		if rec == nil {
			continue
		}
		switch {
		case rec.Phase == PhaseLive:
			f.finish(rec, completed.Result)
		case rec.Phase == PhaseFinishing && rec.step < stepVerify && rec.task != taskStop && rec.task != taskDrop:
			// The computer is not disposed of yet, so the job's result still decides what happens to
			// it. A result replaces one of the helper's own reasons; between two job results the one
			// that keeps wins.
			if isOwnReason(rec.Result) || keepResult(completed.Result) {
				rec.Result = completed.Result
				f.persist(rec)
			}
		}
	}
	if m.assigned != nil {
		set.assigned = *m.assigned
	}
}

func (f *Fleet) onExited(ev runnerExited, now time.Time) {
	rec := f.records[ev.name]
	f.opts.Logger.Info("runner command ended", "runner", ev.name)
	if rec == nil || rec.Phase != PhaseLive || (ev.proc != nil && rec.proc != ev.proc) {
		return
	}
	switch rec.sub {
	case subIdle:
		// The runner program ended before any job reached it.
		f.finish(rec, resultExited)
	case subBusy:
		// Its job should report; if it does not, GitHub is asked after a grace period.
		rec.proc, rec.notBefore = nil, now.Add(exitGrace)
	}
}

func (f *Fleet) onForkDone(rec *record, ev forkDone, now time.Time) {
	if rec == nil {
		return
	}
	set := f.sets[rec.Label]
	if ev.nameTaken {
		// The engine refused the name, so it made nothing and the computer that has the name is not
		// the helper's: forget the record and delete nothing.
		f.opts.Logger.Warn("cannot fork a computer: its name is taken; leaving the computer that has it", "computer", rec.Name, "error", ev.err.Error())
		f.forget(rec)
		f.noteFailure(set, now)
		return
	}
	if ev.err != nil {
		// A fork that failed some other way (a timeout, a killed command, unreadable output) may
		// still have made the computer. The record stays until a list started from now on binds it
		// by its exact name, so it is deleted, or shows no such computer, so it is forgotten.
		f.opts.Logger.Warn("cannot fork a computer", "computer", rec.Name, "error", ev.err.Error())
		rec.seenFrom = f.listStarts + 1
		f.noteFailure(set, now)
		return
	}
	rec.ID, rec.Phase, rec.sub, rec.since, rec.seenFrom = ev.id, PhaseLive, subStarting, now, f.listStarts+1
	f.persist(rec)
}

func (f *Fleet) onUpDone(rec *record, ev upDone, now time.Time) {
	if rec == nil {
		return
	}
	set := f.sets[rec.Label]
	if ev.proc != nil {
		// The command is drained and waited for until it ends, whether or not the runner came up:
		// one that never listened is still running, and nothing else would reap it.
		rec.proc = ev.proc
		f.watch(rec.Name, ev.proc)
	}
	if ev.err != nil {
		f.opts.Logger.Warn("runner did not come up", "computer", rec.Name, "error", ev.err.Error())
		f.noteFailure(set, now)
		if rec.Phase == PhaseLive {
			f.finish(rec, resultNotStarted)
		}
		return
	}
	set.succeeded()
	// The computer runs now: a list that started before the start says nothing about it.
	rec.seenFrom = f.listStarts + 1
	if rec.Phase == PhaseLive && rec.sub == subStarting {
		rec.sub, rec.since = subIdle, now
	}
	f.opts.Logger.Info("runner ready", "runner", rec.Name, "label", rec.Label)
}

func (f *Fleet) noteFailure(set *setState, now time.Time) {
	if set == nil {
		return
	}
	if wait := set.failed(now, f.opts.StartAttempts); wait > 0 {
		f.opts.Logger.Error("no runner could be started; waiting before trying again", "label", set.spec.Label, "wait", wait.String())
	}
}

func (f *Fleet) onDeregisterDone(rec *record, ev deregisterDone, now time.Time) {
	if rec == nil {
		return
	}
	switch {
	case ev.err != nil:
		f.opts.Logger.Warn("cannot deregister the runner", "computer", rec.Name, "error", ev.err.Error())
		rec.notBefore = now.Add(retryAfter)
	case ev.jobRunning:
		// The runner has a job: the computer stays as it is and its result comes with the job. A
		// job result already received is kept for when the runner is gone; the helper's own
		// reasons end with the runner they were about, and the job's result is not known yet, so
		// a restart before JobStarted is persisted still adopts the computer as busy.
		f.opts.Logger.Info("the runner's job is still running; keeping it", "runner", rec.Name)
		if isOwnReason(rec.Result) {
			rec.Result = resultUnknown
		}
		// Its command is no longer watched (it may be dead already); GitHub is asked now and then.
		rec.Phase, rec.sub, rec.since, rec.proc, rec.notBefore = PhaseLive, subBusy, now, nil, now.Add(probeEvery)
		f.persist(rec)
	default:
		if rec.proc != nil {
			rec.proc.Kill()
			rec.proc = nil
		}
		rec.step = stepDispose
	}
}

func (f *Fleet) onStopDone(rec *record, ev stopDone, now time.Time) {
	if rec == nil {
		return
	}
	if ev.err != nil {
		f.opts.Logger.Warn("cannot stop the computer to keep it", "computer", rec.Name, "error", ev.err.Error())
		rec.notBefore = now.Add(retryAfter)
		return
	}
	rec.Phase, rec.At, rec.step, rec.notBefore = PhaseKept, now, stepDispose, time.Time{}
	f.persist(rec)
	f.opts.Logger.Info("kept a computer after a job that did not succeed", "computer", rec.Name, "result", rec.Result)
}

func (f *Fleet) onDropDone(rec *record, ev dropDone, now time.Time) {
	if rec == nil {
		return
	}
	// Whether the drop worked or not, the next fresh list says what is left.
	rec.step, rec.seenFrom, rec.notBefore = stepVerify, f.listStarts+1, time.Time{}
	if ev.err != nil {
		f.opts.Logger.Warn("cannot delete the computer", "computer", rec.Name, "error", ev.err.Error())
		rec.notBefore = now.Add(retryAfter)
	}
}

func (f *Fleet) onProbeDone(rec *record, ev probeDone, now time.Time) {
	if rec == nil {
		return
	}
	rec.notBefore = now.Add(probeEvery)
	if ev.err != nil {
		f.opts.Logger.Warn("cannot look the runner up", "computer", rec.Name, "error", ev.err.Error())
		return
	}
	if !ev.present && rec.Phase == PhaseLive {
		// The job ended: with the result GitHub already gave, or one nobody saw.
		result := rec.Result
		if result == "" {
			result = resultUnknown
		}
		f.opts.Logger.Info("a runner's job has ended", "computer", rec.Name, "result", result)
		f.finish(rec, result)
	}
}

// onListed applies a list: it binds the computers of records that forked while the helper was not
// watching, and forgets computers that a fresh list no longer shows.
func (f *Fleet) onListed(ev listDone, now time.Time) {
	if ev.err != nil {
		f.opts.Logger.Warn("cannot list the computers", "error", ev.err.Error())
		f.listNotBefore = now.Add(retryAfter)
		return
	}
	f.list, f.listSeq, f.fresh = ev.computers, ev.seq, true
	byID := map[string]bool{}
	byName := map[string]Computer{}
	for _, computer := range ev.computers {
		byID[computer.ID] = true
		byName[computer.Name] = computer
	}
	for _, rec := range f.sortedRecords() {
		if rec.task != taskNone || ev.seq < rec.seenFrom {
			continue
		}
		if rec.Phase == PhaseForking {
			// No fork is running for this record: it failed, or was left by a run that stopped before
			// it answered. The name was chosen and recorded by the helper: if a computer has it,
			// that is the one; if none does, the fork never happened.
			computer, ok := byName[rec.Name]
			if !ok {
				f.forget(rec)
				continue
			}
			rec.ID, rec.Phase, rec.seenFrom = computer.ID, PhaseLive, ev.seq+1
			f.finish(rec, resultOrphan)
			continue
		}
		if rec.ID == "" {
			continue
		}
		switch {
		case byID[rec.ID] && rec.step == stepVerify && (rec.Phase == PhaseFinishing || rec.Phase == PhaseKept):
			// It is still there: the drop did not take.
			rec.step = stepDispose
		case !byID[rec.ID] && (rec.Phase == PhaseKept || (rec.Phase == PhaseFinishing && rec.step >= stepDispose)):
			f.opts.Logger.Info("a computer is gone", "computer", rec.Name)
			f.forget(rec)
		}
	}
}

// watch drains the runner's output and reports when its command ends.
func (f *Fleet) watch(name string, proc Process) {
	go func() {
		for range proc.Lines() {
		}
		_ = proc.Wait()
		f.post(runnerExited{name: name, proc: proc})
	}()
}

// MARK: plan

func (f *Fleet) sortedRecords() []*record {
	recs := make([]*record, 0, len(f.records))
	for _, rec := range f.records {
		recs = append(recs, rec)
	}
	sort.Slice(recs, func(i, j int) bool { return recs[i].Name < recs[j].Name })
	return recs
}

func (f *Fleet) labels() []string {
	labels := make([]string, 0, len(f.sets))
	for label := range f.sets {
		labels = append(labels, label)
	}
	sort.Strings(labels)
	return labels
}

// plan starts the next step of every record that has none in flight, then scales. It runs after
// every event and is the only place work is started.
func (f *Fleet) plan(tick bool) {
	now := f.opts.Now()
	needList := tick && len(f.records) > 0
	for _, rec := range f.sortedRecords() {
		if rec.task == taskNone {
			needList = f.advance(rec, now) || needList
		}
	}
	f.retain(now)
	needList = f.scale(now) || needList
	if needList && !f.listBusy && !now.Before(f.listNotBefore) {
		f.startList()
	}
}

// advance starts the next step of one record. It reports whether it needs a list.
func (f *Fleet) advance(rec *record, now time.Time) (needList bool) {
	set := f.sets[rec.Label]
	switch rec.Phase {
	case PhaseForking:
		return true
	case PhaseLive:
		if set == nil {
			return false
		}
		if f.closing {
			if rec.sub == subStarting || rec.idle() {
				f.finish(rec, resultShutdown)
				return f.advance(rec, now)
			}
			return false
		}
		switch rec.sub {
		case subStarting:
			f.startUp(rec, set)
		case subBusy:
			if rec.proc == nil && !now.Before(rec.notBefore) {
				name := rec.Name
				f.spawn(rec, taskProbe, func() any {
					ref, err := f.gh.GetRunnerByName(f.ctx, name)
					return probeDone{name: name, present: ref != nil, err: err}
				})
			}
		}
	case PhaseFinishing:
		if rec.step == stepVerify {
			return true
		}
		if now.Before(rec.notBefore) {
			return false
		}
		switch rec.step {
		case stepDeregister:
			f.startDeregister(rec)
		case stepDispose:
			if f.keeps(rec) {
				id, name := rec.ID, rec.Name
				f.spawn(rec, taskStop, func() any { return stopDone{name: name, err: f.silo.Stop(f.ctx, id)} })
			} else {
				f.startDrop(rec)
			}
		}
	case PhaseKept:
		return rec.step == stepVerify
	}
	return false
}

// keeps says whether a finished computer is stopped and kept instead of deleted: its result keeps
// it and its scale set keeps any.
func (f *Fleet) keeps(rec *record) bool {
	if !keepResult(rec.Result) {
		return false
	}
	set := f.sets[rec.Label]
	return set == nil || set.spec.KeepFailed.Limit() > 0
}

// spawn runs a task for a record. Its result comes back as an event; until then the record has
// no other task.
func (f *Fleet) spawn(rec *record, kind taskKind, run func() any) {
	rec.task = kind
	f.inflight++
	go func() { f.post(run()) }()
}

func (f *Fleet) startDeregister(rec *record) {
	name := rec.Name
	f.spawn(rec, taskDeregister, func() any {
		ref, err := f.gh.GetRunnerByName(f.ctx, name)
		if err != nil {
			return deregisterDone{name: name, err: err}
		}
		if ref == nil {
			return deregisterDone{name: name}
		}
		switch err := f.gh.RemoveRunner(f.ctx, int64(ref.ID)); {
		case err == nil, errors.Is(err, scaleset.RunnerNotFoundError):
			return deregisterDone{name: name}
		case errors.Is(err, scaleset.JobStillRunningError):
			return deregisterDone{name: name, jobRunning: true}
		default:
			return deregisterDone{name: name, err: err}
		}
	})
}

func (f *Fleet) startDrop(rec *record) {
	id, name := rec.ID, rec.Name
	f.spawn(rec, taskDrop, func() any { return dropDone{name: name, err: f.silo.Drop(f.ctx, id)} })
}

func (f *Fleet) startUp(rec *record, set *setState) {
	id, name, argv, scaleSetID := rec.ID, rec.Name, set.start, set.id
	f.spawn(rec, taskUp, func() any {
		ctx := f.upCtx
		if err := f.silo.Start(ctx, id); err != nil {
			return upDone{name: name, err: err}
		}
		jit, err := f.gh.GenerateJitRunnerConfig(ctx, &scaleset.RunnerScaleSetJitRunnerSetting{Name: name, WorkFolder: "_work"}, scaleSetID)
		if err != nil {
			return upDone{name: name, err: fmt.Errorf("cannot generate the runner configuration: %w", err)}
		}
		proc, err := f.silo.StartRunner(ctx, id, jit.EncodedJITConfig, argv)
		if err != nil {
			return upDone{name: name, err: err}
		}
		return upDone{name: name, proc: proc, err: waitReady(ctx, proc, f.opts.ReadyTimeout)}
	})
}

func (f *Fleet) startList() {
	f.listStarts++
	seq := f.listStarts
	f.listBusy = true
	f.inflight++
	go func() {
		computers, err := f.silo.List(f.ctx)
		f.post(listDone{seq: seq, computers: computers, err: err})
	}()
}

// retain deletes kept computers older than keepFailed.days, then the oldest ones beyond
// keepFailed.count. It only ever looks at records in the kept phase.
func (f *Fleet) retain(now time.Time) {
	for _, label := range f.labels() {
		spec := f.sets[label].spec
		var kept []*record
		for _, rec := range f.sortedRecords() {
			if rec.Label == label && rec.Phase == PhaseKept && rec.step != stepVerify && rec.task != taskDrop {
				kept = append(kept, rec)
			}
		}
		sort.SliceStable(kept, func(i, j int) bool { return kept[i].At.Before(kept[j].At) })
		var excess []*record
		var live []*record
		for _, rec := range kept {
			if now.Sub(rec.At) > time.Duration(spec.KeepFailed.Days)*24*time.Hour {
				excess = append(excess, rec)
			} else {
				live = append(live, rec)
			}
		}
		for len(live) > spec.KeepFailed.Limit() {
			excess, live = append(excess, live[0]), live[1:]
		}
		for _, rec := range excess {
			if rec.task == taskNone && !now.Before(rec.notBefore) {
				f.opts.Logger.Info("deleting a kept computer", "computer", rec.Name)
				f.startDrop(rec)
			}
		}
	}
}

// scale starts runners for the jobs that have none and removes idle ones no job needs.
func (f *Fleet) scale(now time.Time) (needList bool) {
	for _, label := range f.labels() {
		set := f.sets[label]
		target := min(max(set.assigned, len(set.outstanding)), set.spec.MaxRunners)
		have, idle := 0, 0
		for _, rec := range f.records {
			if rec.Label != label || (rec.Phase != PhaseForking && rec.Phase != PhaseLive) {
				continue
			}
			have++
			if rec.idleFor(now, f.opts.IdleTimeout) {
				idle++
			}
		}
		if f.closing {
			continue
		}
		for _, rec := range f.sortedRecords() {
			if have <= target || idle == 0 {
				break
			}
			if rec.Label == label && rec.idleFor(now, f.opts.IdleTimeout) {
				f.opts.Logger.Info("removing an idle runner no job needs", "runner", rec.Name)
				f.finish(rec, resultIdle)
				f.advance(rec, now)
				have--
				idle--
			}
		}
		if have >= target || now.Before(set.backoffTill) {
			continue
		}
		if !f.fresh {
			needList = true
			continue
		}
		for ; have < target; have++ {
			if err := f.disk.Check(); err != nil {
				f.opts.Logger.Info("not starting a computer now", "label", label, "reason", err.Error())
				break
			}
			memory := RecipeMemoryGiB(f.opts.ProjectDir, set.spec.Recipe, set.spec.OS)
			if err := (Capacity{Linux: f.opts.Linux}).Allows(set.spec.OS, memory, f.list, f.pendingDemands(f.list, f.listSeq)); err != nil {
				f.opts.Logger.Info("not starting a computer now", "label", label, "reason", err.Error())
				break
			}
			f.startFork(set, now)
		}
	}
	return needList
}

// pendingDemands are the computers asked for that the list does not show running: every record
// that is forking or live (a list may be stale, and counting a computer twice is the safe
// direction) and the recipe rebuilds in progress. A live record is the exception when listSeq, the
// number of the list, says that list started after the record's fork or last start and shows its
// computer stopped or gone: that computer holds no slot, and it is not about to (it is neither
// starting nor has a start in flight, and no runner command of this run is watched in it). A negative listSeq is a list that cannot be placed in
// that order, so nothing is discounted.
func (f *Fleet) pendingDemands(listed []Computer, listSeq int) []Demand {
	byID, byName := map[string]bool{}, map[string]bool{}
	for _, computer := range listed {
		if computer.Occupies() {
			byID[computer.ID] = true
			byName[computer.Name] = true
		}
	}
	var pending []Demand
	for _, rec := range f.sortedRecords() {
		set := f.sets[rec.Label]
		if set == nil || (rec.Phase != PhaseForking && rec.Phase != PhaseLive) {
			continue
		}
		if (rec.ID != "" && byID[rec.ID]) || (rec.ID == "" && byName[rec.Name]) {
			continue
		}
		if rec.Phase == PhaseLive && rec.sub != subStarting && rec.task != taskUp && rec.proc == nil && listSeq >= rec.seenFrom && listSeq >= 0 {
			continue
		}
		pending = append(pending, Demand{OS: set.spec.OS, MemoryGiB: RecipeMemoryGiB(f.opts.ProjectDir, set.spec.Recipe, set.spec.OS)})
	}
	for _, demand := range f.rebuilds {
		pending = append(pending, demand)
	}
	return pending
}

// startFork records a computer by name and forks it. The record comes first, so a crash at any
// later point leaves it.
func (f *Fleet) startFork(set *setState, now time.Time) {
	taken := map[string]bool{}
	for _, computer := range f.list {
		taken[computer.Name] = true
	}
	name := ComputerName(set.spec.Label, f.opts.NewID())
	for taken[name] || f.records[name] != nil {
		name = ComputerName(set.spec.Label, f.opts.NewID())
	}
	rec := &record{Entry: Entry{Name: name, Label: set.spec.Label, Phase: PhaseForking, At: now}}
	if err := f.ledger.Put(rec.Entry); err != nil {
		f.opts.Logger.Error("cannot record the computer before forking it", "computer", name, "error", err.Error())
		_ = f.ledger.Remove(name)
		return
	}
	f.records[name] = rec
	recipe := set.spec.Recipe
	f.spawn(rec, taskFork, func() any {
		gate := f.gates[recipe]
		gate.RLock()
		defer gate.RUnlock()
		id, err := f.silo.Fork(f.ctx, name, recipe)
		return forkDone{name: name, id: id, err: err, nameTaken: errors.Is(err, errNameTaken)}
	})
}
