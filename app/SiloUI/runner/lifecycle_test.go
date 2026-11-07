package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/actions/scaleset"
)

// These tests are the regressions of the review rounds on the lifecycle (docs/forks-ci/R1-SIMPLER-DESIGN.md),
// plus what a restart does. Every one is driven through events and settles without sleeping.

// MARK: 1. retention only touches a computer that is kept

func TestRetentionNeverTouchesAComputerBeforeItsRunnerIsDeregistered(t *testing.T) {
	h := newHarness(t, harnessOptions{spec: ScaleSetSpec{Label: "silo-linux", Recipe: "silo-linux", OS: "linux", MaxRunners: 3, KeepFailed: KeepFailed{Count: intp(1), Days: 7}}})
	// The first failed job's runner cannot be deregistered: GitHub is failing for it.
	h.scale(available(1))
	first := h.only("idle")
	h.actions.removeErr[h.actions.idOf(first)] = errors.New("GitHub is down")
	h.scale(completed(1, first, "failed", 0))
	for request := int64(2); request <= 3; request++ {
		h.advance(time.Minute)
		h.scale(available(request))
		name := h.only("idle")
		h.scale(completed(request, name, "failed", 0))
	}
	// Two computers are kept and the count is 1: the older of them goes. The first one is not kept
	// yet, so retention cannot see it.
	if state, ok := h.silo.state(first); !ok || state != "running" {
		t.Fatalf("a computer whose runner is still registered was touched: %q %v (%v)", state, ok, h.log.all())
	}
	if h.log.count("stop "+first) != 0 || h.log.count("drop "+first) != 0 {
		t.Fatalf("events %v", h.log.all())
	}
	if len(h.silo.names()) != 2 {
		t.Fatalf("computers = %v", h.silo.names())
	}
	// Days do not reach it either.
	h.later(30 * 24 * time.Hour)
	if state, ok := h.silo.state(first); !ok || state != "running" {
		t.Fatalf("days deleted a computer that is not kept: %q %v", state, ok)
	}
	// Once GitHub answers, it is deregistered and kept like any failure.
	delete(h.actions.removeErr, h.actions.idOf(first))
	h.later(time.Minute)
	if state, _ := h.silo.state(first); state != "stopped" || h.status()[first] != "kept" {
		t.Fatalf("it should be kept now: %q %v", state, h.status())
	}
}

// MARK: 2. a computer is ours by id, never by name

func TestACleanupNeverDeletesAComputerThatHasTheSameNameButAnotherID(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	// The helper's computer disappears and someone creates another with the same name.
	id := h.silo.idOf(name)
	h.silo.mu.Lock()
	delete(h.silo.computers, id)
	h.silo.mu.Unlock()
	h.silo.add(Computer{ID: "twin", Name: name, Kind: "linux", State: "running"})
	h.scale(completed(1, name, "succeeded", 0))
	if state, ok := h.silo.state(name); !ok || state != "running" {
		t.Fatalf("the twin was touched: %q %v (%v)", state, ok, h.log.all())
	}
	if h.log.count("drop "+name) != 0 || len(h.ledger.All()) != 0 {
		t.Fatalf("events %v ledger %v", h.log.all(), h.ledger.All())
	}
}

// A fork that answers with an error (a timeout, unreadable output) may still have made the
// computer. Its name was chosen and recorded by the helper, so a computer that has it is the
// helper's: it is bound by that exact name and deleted by id. Other computers are not touched.
func TestAForkThatFailsAfterMakingTheComputerDeletesItByItsName(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "main", Name: "main", Kind: "linux", State: "running"})
	h.silo.failFork = true
	h.silo.forkHook = func(name string) {
		h.silo.add(Computer{ID: "made-" + name, Name: name, Kind: "linux", State: "stopped"})
	}
	h.scale(available(1))
	if n := h.log.count("fork "); n != 3 {
		t.Fatalf("forks = %d", n)
	}
	if names := h.silo.names(); len(names) != 1 || names[0] != "main" {
		t.Fatalf("computers %v events %v", names, h.log.all())
	}
	if h.log.count("drop ci-") != 3 || h.log.count("drop main") != 0 {
		t.Fatalf("events %v", h.log.all())
	}
	if len(h.status()) != 0 || len(h.ledger.All()) != 0 {
		t.Fatalf("records %v", h.status())
	}
}

// The engine refuses a name another computer has and makes nothing. The computer that has the name
// is not the helper's, so the record is forgotten at once and nothing is deleted.
func TestAForkRefusedBecauseTheNameIsTakenForgetsTheRecordAndDeletesNothing(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.forkErr = fmt.Errorf("%w: silo fork: Computer names must be unique.", errNameTaken)
	// A computer with the chosen name appears between the helper's list and its fork.
	h.silo.forkHook = func(name string) {
		h.silo.add(Computer{ID: "theirs-" + name, Name: name, Kind: "linux", State: "stopped"})
	}
	h.scale(available(1))
	h.later(time.Minute)
	if h.log.count("fork ") < 1 || len(h.status()) != 0 || len(h.ledger.All()) != 0 {
		t.Fatalf("the record is forgotten: %v %v", h.status(), h.log.all())
	}
	if h.log.count("drop ") != 0 || h.log.count("deregister") != 0 || len(h.silo.names()) != h.log.count("fork ") {
		t.Fatalf("a computer the helper did not make was touched: %v (%v)", h.silo.names(), h.log.all())
	}
}

func TestAForkThatFailsKeepsItsRecordUntilAFreshListAnswers(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.failFork = true
	// The fork fails and the computers cannot be listed: nothing says whether a computer was made.
	h.silo.forkHook = func(string) {
		h.silo.mu.Lock()
		h.silo.listErr = errors.New("engine is busy")
		h.silo.mu.Unlock()
	}
	h.scale(available(1))
	name := h.only("forking")
	if _, ok := h.ledger.Get(name); !ok || h.log.count("fork ") != 1 {
		t.Fatalf("the record stays while its computer is unknown: %v", h.log.all())
	}
	h.silo.mu.Lock()
	h.silo.listErr, h.silo.failFork, h.silo.forkHook = nil, false, nil
	h.silo.mu.Unlock()
	h.later(time.Minute)
	if _, ok := h.ledger.Get(name); ok || h.log.count("drop ") != 0 {
		t.Fatalf("a list that shows no such computer forgets the record: %v", h.log.all())
	}
	if h.log.count("fork ") != 2 || len(h.status()) != 1 {
		t.Fatalf("a replacement is forked once the failed one is settled: %v %v", h.status(), h.log.all())
	}
}

func TestEveryCallAfterTheForkGoesByID(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(completed(1, name, "failed", 0))
	for _, line := range h.log.all() {
		if len(line) > 2 && line[:2] == "?i" {
			t.Fatalf("a call named a computer the engine does not know by id: %s", line)
		}
	}
	// The fake only answers to ids, so a stop that worked went by id.
	if state, _ := h.silo.state(name); state != "stopped" {
		t.Fatalf("state = %q (%v)", state, h.log.all())
	}
}

// MARK: 3. absence comes from a fresh, successful list only

func TestARenamedComputerIsStillOursAndAFailedListForgetsNothing(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(completed(1, name, "failed", 0))
	id := h.silo.idOf(name)
	// The owner renames the kept computer.
	h.silo.mu.Lock()
	c := h.silo.computers[id]
	c.Name = "my-debugging-copy"
	h.silo.computers[id] = c
	h.silo.mu.Unlock()
	h.tick()
	if h.status()[name] != "kept" {
		t.Fatalf("a renamed computer was forgotten: %v", h.status())
	}
	// A list that fails says nothing, even if the computer is really gone.
	h.silo.mu.Lock()
	delete(h.silo.computers, id)
	h.silo.listErr = errors.New("engine is not running")
	h.silo.mu.Unlock()
	h.later(time.Minute)
	if h.status()[name] != "kept" {
		t.Fatalf("a failed list forgot a computer: %v", h.status())
	}
	h.silo.mu.Lock()
	h.silo.listErr = nil
	h.silo.mu.Unlock()
	h.later(time.Minute)
	if len(h.status()) != 0 || len(h.ledger.All()) != 0 {
		t.Fatalf("a fresh list that lacks the id should forget it: %v", h.status())
	}
}

func TestRetentionDeletesARenamedKeptComputerByID(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(completed(1, name, "failed", 0))
	id := h.silo.idOf(name)
	h.silo.mu.Lock()
	c := h.silo.computers[id]
	c.Name = "renamed"
	h.silo.computers[id] = c
	h.silo.mu.Unlock()
	h.later(8 * 24 * time.Hour)
	if _, ok := h.silo.state("renamed"); ok || len(h.ledger.All()) != 0 {
		t.Fatalf("computers %v ledger %v", h.silo.names(), h.ledger.All())
	}
}

// MARK: 4. a failure is retried by the tick, not left until the next start

func TestAGitHubOutageAfterARestartIsRetriedAndFreesBothMacSlots(t *testing.T) {
	spec := ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 2, KeepFailed: KeepFailed{Count: intp(1), Days: 1}}
	h := newHarness(t, harnessOptions{spec: spec})
	for _, name := range []string{"ci-silo-macos-aaaaaaaa", "ci-silo-macos-bbbbbbbb"} {
		h.silo.add(Computer{ID: "id-" + name, Name: name, Kind: "macos", State: "running"})
		h.actions.runners[name] = 50 + len(h.actions.runners)
	}
	h.actions.lookupErr = errors.New("GitHub is down")
	h.restart(
		Entry{Name: "ci-silo-macos-aaaaaaaa", ID: "id-ci-silo-macos-aaaaaaaa", Label: "silo-macos", Phase: PhaseLive, At: h.clock()},
		Entry{Name: "ci-silo-macos-bbbbbbbb", ID: "id-ci-silo-macos-bbbbbbbb", Label: "silo-macos", Phase: PhaseLive, At: h.clock()},
	)
	h.scale(available(1, 2))
	if h.log.count("fork ") != 0 || len(h.silo.names()) != 2 {
		t.Fatalf("both slots are held and nothing can be asked of GitHub: %v", h.log.all())
	}
	h.actions.lookupErr = nil
	h.later(time.Minute)
	h.scale(available(1, 2))
	if h.log.count("fork ") != 2 {
		t.Fatalf("the retry should have freed both slots: %v (%v)", h.log.all(), h.status())
	}
	for _, old := range []string{"ci-silo-macos-aaaaaaaa", "ci-silo-macos-bbbbbbbb"} {
		if _, ok := h.silo.state(old); ok {
			t.Fatalf("%s should be deleted", old)
		}
	}
}

// MARK: 5. a failed ledger write loses nothing

func TestAFailedLedgerWriteWhenTheIDIsBoundIsRecoveredByTheNextStart(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	calls := 0
	h.ledger.sync = func(f *os.File) error {
		calls++
		// The first save is the record before the fork (two flushes); the second is the id.
		if calls == 3 {
			return errors.New("disk full")
		}
		return f.Sync()
	}
	h.scale(available(1))
	name := h.onlyRunner()
	if entry, _ := h.ledger.Get(name); entry.ID == "" {
		t.Fatal("the ledger in memory should know the id; the write is what failed")
	}
	// The process dies now. What is on disk is the record from before the fork.
	h.restart()
	if _, ok := h.silo.state(name); ok {
		t.Fatalf("the computer must be found by name, bound and deleted: %v", h.log.all())
	}
	if len(h.ledger.All()) != 0 || h.actions.idOf(name) != 0 {
		t.Fatalf("ledger %v", h.ledger.All())
	}
}

func TestAFailedLedgerWriteDoesNotStopTheComputerBeingCleanedUp(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.ledger.sync = func(f *os.File) error { return errors.New("disk full") }
	h.scale(completed(1, name, "succeeded", 0))
	if _, ok := h.silo.state(name); ok || h.status()[name] != "" {
		t.Fatalf("the computer should be deleted although nothing could be written: %v (%v)", h.status(), h.log.all())
	}
}

// MARK: 6. a result that arrives while the computer is finishing counts

func TestAFailedResultArrivingWhileFinishingKeepsTheComputer(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.output = func(n int) []string {
		if n == 0 {
			return []string{"nothing useful"}
		}
		return []string{"Listening for Jobs"}
	}
	first := "ci-silo-linux-00000001"
	h.actions.removeErr[101] = errors.New("GitHub is down")
	h.scale(available(1))
	if h.status()[first] != "finishing" {
		t.Fatalf("runners = %v", h.status())
	}
	// The runner did register and a job did run on it; its failure arrives while the start's
	// clean-up waits for GitHub.
	h.scale(completed(1, first, "failed", 1))
	delete(h.actions.removeErr, 101)
	h.later(time.Minute)
	if state, ok := h.silo.state(first); !ok || state != "stopped" || h.status()[first] != "kept" {
		t.Fatalf("a failed job's computer was not kept: %q %v %v (%v)", state, ok, h.status(), h.log.all())
	}
}

func TestASuccessArrivingAfterAnUnknownResultIsNotKeptAsUnknown(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	h.exited(name)
	h.actions.forget(name)
	h.silo.stopErr = errors.New("busy")
	h.later(3 * time.Minute) // GitHub has no runner: the result is unknown, and stopping the computer fails
	entry, _ := h.ledger.Get(name)
	if entry.Phase != PhaseFinishing || entry.Result != resultUnknown {
		t.Fatalf("entry = %+v", entry)
	}
	h.scale(completed(1, name, "succeeded", 0))
	entry, _ = h.ledger.Get(name)
	if entry.Result != "succeeded" {
		t.Fatalf("the job's result replaces the helper's guess: %+v", entry)
	}
	h.silo.stopErr = nil
	h.later(time.Minute)
	if _, ok := h.silo.state(name); ok {
		t.Fatalf("a succeeded job's computer is deleted: %v", h.log.all())
	}
}

func TestAKeepingResultIsNotReplacedByASuccess(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.actions.removeErr[h.actions.idOf(name)] = errors.New("GitHub is down")
	h.scale(completed(1, name, "failed", 0))
	h.scale(completed(1, name, "succeeded", 0))
	if entry, _ := h.ledger.Get(name); entry.Result != "failed" {
		t.Fatalf("entry = %+v", entry)
	}
}

// MARK: 7. the recipe decides how much memory a computer needs

func TestCapacityUsesTheMemoryOfTheRecipeFile(t *testing.T) {
	h := newHarness(t, harnessOptions{linux: LinuxBudget{MemoryGiB: 20}, recipeMemory: 16})
	h.silo.add(Computer{Name: "main", Kind: "linux", State: "running", MemoryGiB: 8})
	h.scale(available(1))
	if h.log.count("fork ") != 0 {
		t.Fatalf("8 GiB are in use and the recipe asks for 16 of a 20 GiB budget: %v", h.log.all())
	}
	// The recipe file is read when the decision is made.
	h.writeRecipe("silo-linux", 8)
	h.scale(nil)
	if h.log.count("fork ") != 1 {
		t.Fatalf("with an 8 GiB recipe it fits: %v", h.log.all())
	}
}

// MARK: a restart is not special

func TestAJobInFlightAcrossARestartIsAdoptedAndKeptAsUnknown(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	h.actions.removeErr[h.actions.idOf(name)] = scaleset.JobStillRunningError
	h.restart()
	if h.status()[name] != "busy" {
		t.Fatalf("a computer whose runner has a job is adopted as busy: %v", h.status())
	}
	if state, _ := h.silo.state(name); state != "running" || h.log.count("drop ") != 0 {
		t.Fatalf("it must keep running: %q (%v)", state, h.log.all())
	}
	// The job ends while no helper is watching: GitHub no longer has the runner.
	delete(h.actions.removeErr, h.actions.idOf(name))
	h.actions.forget(name)
	h.later(time.Minute)
	entry, _ := h.ledger.Get(name)
	if state, _ := h.silo.state(name); state != "stopped" || entry.Phase != PhaseKept || entry.Result != resultUnknown {
		t.Fatalf("it ends with an unknown result and is kept: %q %+v (%v)", state, entry, h.log.all())
	}
}

func TestAnIdleRunnerFromBeforeARestartIsDeregisteredAndDeleted(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	id := h.actions.idOf(name)
	h.restart()
	if _, ok := h.silo.state(name); ok || len(h.ledger.All()) != 0 {
		t.Fatalf("an orphan is removed: %v", h.log.all())
	}
	if h.log.index(fmt.Sprintf("deregister %d", id)) > h.log.index("drop "+name) {
		t.Fatalf("deregister before delete: %v", h.log.all())
	}
}

func TestRecordsLeftBeforeTheForkAnsweredAreBoundByNameOrForgotten(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "x1", Name: "ci-silo-linux-debug", Kind: "linux", State: "running"})    // an owner's, by hand
	h.silo.add(Computer{ID: "x2", Name: "ci-silo-linux-ffffffff", Kind: "linux", State: "stopped"}) // not in the record
	h.silo.add(Computer{ID: "main", Name: "main", Kind: "linux", State: "running"})
	h.silo.add(Computer{ID: "y1", Name: "ci-silo-linux-nofork00", Kind: "linux", State: "stopped"}) // forked, id never recorded
	h.restart(
		Entry{Name: "ci-silo-linux-nofork00", Label: "silo-linux", Phase: PhaseForking, At: h.clock()},
		Entry{Name: "ci-silo-linux-never000", Label: "silo-linux", Phase: PhaseForking, At: h.clock()}, // the fork never happened
	)
	for _, stay := range []string{"ci-silo-linux-debug", "ci-silo-linux-ffffffff", "main"} {
		if _, ok := h.silo.state(stay); !ok {
			t.Errorf("%s is not the helper's and must stay", stay)
		}
	}
	if _, ok := h.silo.state("ci-silo-linux-nofork00"); ok {
		t.Error("a computer recorded before its fork answered is an orphan and must go")
	}
	if len(h.ledger.All()) != 0 || h.log.count("drop ?") != 0 {
		t.Fatalf("ledger = %v", h.ledger.All())
	}
}

func TestKeptComputersStayAndAComputerThatIsGoneIsForgotten(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "id-d", Name: "ci-silo-linux-dddddddd", Kind: "linux", State: "stopped"})
	h.restart(
		Entry{Name: "ci-silo-linux-dddddddd", ID: "id-d", Label: "silo-linux", Phase: PhaseKept, Result: "failed", At: h.clock()},
		Entry{Name: "ci-silo-linux-gone0000", ID: "id-g", Label: "silo-linux", Phase: PhaseKept, Result: "failed", At: h.clock()},
	)
	if _, ok := h.silo.state("ci-silo-linux-dddddddd"); !ok {
		t.Fatal("a kept failure must stay")
	}
	if _, ok := h.ledger.Get("ci-silo-linux-gone0000"); ok || h.status()["ci-silo-linux-dddddddd"] != "kept" {
		t.Fatalf("ledger = %+v", h.ledger.All())
	}
	if h.log.count("drop ") != 0 || h.log.count("deregister ") != 0 {
		t.Fatalf("events %v", h.log.all())
	}
}

func TestAnInterruptedKeepIsFinishedAfterARestart(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "id-a", Name: "ci-silo-linux-aaaaaaaa", Kind: "linux", State: "running"})
	h.actions.runners["ci-silo-linux-aaaaaaaa"] = 21
	h.restart(Entry{Name: "ci-silo-linux-aaaaaaaa", ID: "id-a", Label: "silo-linux", Phase: PhaseFinishing, Result: "failed", At: h.clock()})
	if state, _ := h.silo.state("ci-silo-linux-aaaaaaaa"); state != "stopped" {
		t.Fatalf("state = %q", state)
	}
	if h.log.count("deregister 21") != 1 || h.log.count("drop ") != 0 || h.status()["ci-silo-linux-aaaaaaaa"] != "kept" {
		t.Fatalf("events %v", h.log.all())
	}
}

func TestAComputerWhoseRunnerCannotBeLookedUpIsNotDeleted(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "id-a", Name: "ci-silo-linux-aaaaaaaa", Kind: "linux", State: "running"})
	h.actions.lookupErr = errors.New("offline")
	h.restart(Entry{Name: "ci-silo-linux-aaaaaaaa", ID: "id-a", Label: "silo-linux", Phase: PhaseLive, At: h.clock()})
	if _, ok := h.silo.state("ci-silo-linux-aaaaaaaa"); !ok {
		t.Fatal("deleted a computer without knowing whether its job runs")
	}
}

func TestARecordOfALabelThatIsNoLongerConfiguredIsLeftAlone(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.add(Computer{ID: "id-o", Name: "ci-old-label-aaaaaaaa", Kind: "linux", State: "running"})
	h.restart(Entry{Name: "ci-old-label-aaaaaaaa", ID: "id-o", Label: "old-label", Phase: PhaseLive, At: h.clock()})
	h.tick()
	if _, ok := h.silo.state("ci-old-label-aaaaaaaa"); !ok || h.log.count("drop ") != 0 || h.log.count("stop ") != 0 {
		t.Fatalf("events %v", h.log.all())
	}
}

// MARK: a job that ends while the helper is down, and one GitHub has not released yet

func TestAJobThatEndedWhileTheHelperWasDownIsKeptAsUnknown(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	// The job ends and GitHub removes its runner while no helper is running.
	h.actions.forget(name)
	h.restart()
	entry, ok := h.ledger.Get(name)
	if state, _ := h.silo.state(name); !ok || state != "stopped" || entry.Phase != PhaseKept || entry.Result != resultUnknown {
		t.Fatalf("the computer of a job that may have failed is kept: %q %+v (%v)", state, entry, h.log.all())
	}
	if h.log.count("drop ") != 0 {
		t.Fatalf("events %v", h.log.all())
	}
}

func TestAnIdleRecordWithNoJobAcrossARestartIsStillDeleted(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.actions.forget(name)
	h.restart()
	if _, ok := h.silo.state(name); ok || len(h.ledger.All()) != 0 {
		t.Fatalf("a runner that never had a job is no failure: %v", h.log.all())
	}
}

func TestASuccessWhoseRunnerGitHubHasNotReleasedYetIsDeletedAndNeverKept(t *testing.T) {
	h := newHarness(t, harnessOptions{spec: ScaleSetSpec{Label: "silo-linux", Recipe: "silo-linux", OS: "linux", MaxRunners: 3, KeepFailed: KeepFailed{Count: intp(1), Days: 7}}})
	h.scale(available(1, 2))
	runners := h.status()
	if len(runners) != 2 {
		t.Fatalf("runners = %v", runners)
	}
	var failed, passed string
	for name := range runners {
		failed, passed = passed, name
	}
	h.scale(started(1, failed))
	h.scale(started(2, passed))
	h.actions.removeErr[h.actions.idOf(failed)] = scaleset.JobStillRunningError
	h.actions.removeErr[h.actions.idOf(passed)] = scaleset.JobStillRunningError
	h.scale(completed(1, failed, "failed", 1))
	h.scale(completed(2, passed, "succeeded", 0))
	if h.status()[failed] != "busy" || h.status()[passed] != "busy" {
		t.Fatalf("GitHub has not released the runners yet: %v", h.status())
	}
	// GitHub releases both runners.
	h.actions.forget(failed)
	h.actions.forget(passed)
	h.later(time.Minute)
	if _, ok := h.silo.state(passed); ok {
		t.Fatalf("a succeeded job is deleted: %v", h.log.all())
	}
	entry, ok := h.ledger.Get(failed)
	if state, _ := h.silo.state(failed); !ok || state != "stopped" || entry.Phase != PhaseKept || entry.Result != "failed" {
		t.Fatalf("a failed job keeps its own result, not unknown: %q %+v", state, entry)
	}
	if len(h.ledger.All()) != 1 {
		t.Fatalf("only the failure is kept: %+v", h.ledger.All())
	}
}

// MARK: capacity counts what the latest list may not show

func TestAStaleListDoesNotHideRunnersThatAreUpAlready(t *testing.T) {
	h := newHarness(t, harnessOptions{linux: LinuxBudget{Count: 2}})
	h.scale(available(1, 2))
	if len(h.status()) != 2 {
		t.Fatalf("runners = %v", h.status())
	}
	// A cached inventory that shows none of them running.
	h.silo.mu.Lock()
	h.silo.staleList = []Computer{}
	h.silo.mu.Unlock()
	h.scale(available(1, 2, 3))
	if n := h.log.count("fork "); n != 2 {
		t.Fatalf("two Linux computers are up and the limit is two: %d forks (%v)", n, h.log.all())
	}
}

func TestARecipeRebuildCountsAsAComputerWhileItRuns(t *testing.T) {
	h := newHarness(t, harnessOptions{linux: LinuxBudget{MemoryGiB: 12}})
	text := recipeText("2.338.2", strings.Repeat("a", 64))
	if err := os.WriteFile(filepath.Join(h.projectDir, ".silo", "recipes", "silo-linux.yaml"), []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{"actions-runner-linux-arm64-2.339.0.tar.gz": strings.Repeat("b", 64)}}
	refresher := &Refresher{ProjectDir: h.projectDir, Recipes: map[string]string{"silo-linux": "linux"}, Source: fixedRelease(release), Silo: h.silo,
		Reserve: h.fleet.Reserve, Gates: h.fleet.gates, AfterDays: 3, Now: func() time.Time { return time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC) }, Logger: quiet()}
	h.silo.buildHook = func() {
		// The rebuild's VM is not in the list the fleet sees; it still holds 8 of the 12 GiB.
		h.scale(available(1))
		if h.log.count("fork ") != 0 {
			t.Errorf("a computer was forked during the rebuild: %v", h.log.all())
		}
		if _, err := h.fleet.Reserve(context.Background(), "linux", 8); !errors.Is(err, errNoCapacity) {
			t.Errorf("a second rebuild must not fit: %v", err)
		}
	}
	if err := refresher.CheckOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	h.fleet.Settle()
	if h.log.count("build ") != 1 || h.log.count("fork ") != 1 {
		t.Fatalf("the waiting job is forked once the rebuild is over: %v", h.log.all())
	}
}

// A runner removed while it had a job but before JobStarted is persisted has no request yet. Its
// result is unknown, not empty, so a restart adopts it as busy; with the job over meanwhile it is
// kept as unknown instead of being taken for an orphan and deleted.
func TestARestartBeforeJobStartedIsPersistedKeepsAFailedJobsComputer(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	runnerID := h.actions.idOf(name)
	h.actions.removeErr[runnerID] = scaleset.JobStillRunningError
	// The job request is cancelled, so no job needs the idle runner; its job starts meanwhile.
	h.scale(completed(1, "", "canceled", 0))
	h.advance(5 * time.Minute)
	h.tick()
	if h.status()[name] != "busy" {
		t.Fatalf("an idle runner that got a job stays: %v (%v)", h.status(), h.log.all())
	}
	if entry, _ := h.ledger.Get(name); entry.Result != resultUnknown || entry.Request != 0 {
		t.Fatalf("entry = %+v", entry)
	}
	// The helper restarts, and the job ends while it is down.
	delete(h.actions.removeErr, runnerID)
	h.actions.forget(name)
	h.restart()
	entry, _ := h.ledger.Get(name)
	if state, _ := h.silo.state(name); state != "stopped" || entry.Phase != PhaseKept || entry.Result != resultUnknown || h.log.count("drop ") != 0 {
		t.Fatalf("the computer is kept as unknown: %q %+v (%v)", state, entry, h.log.all())
	}
}
