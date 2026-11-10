package main

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/actions/scaleset"
)

func TestAJobGetsAFreshComputerAndRunnerNamedAlike(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	if name != "ci-silo-linux-00000001" {
		t.Fatalf("name = %s", name)
	}
	if got := h.status()[name]; got != "idle" {
		t.Fatalf("state = %s", got)
	}
	// The runner is named exactly as the computer, and the order is fork, start, JIT, runner.
	for i, want := range []string{"fork " + name + " silo-linux", "start " + name, "jit " + name, "runner " + name} {
		if got := h.log.all()[i]; got != want {
			t.Fatalf("event %d = %q, want %q (all: %v)", i, got, want, h.log.all())
		}
	}
	// After the fork every call goes by the computer's id.
	id := h.silo.idOf(name)
	if h.silo.argvs[0][0] != id || h.silo.argvs[0][1] != "/opt/actions-runner/silo-run-jit" {
		t.Fatalf("argv = %v", h.silo.argvs[0])
	}
	entry, ok := h.ledger.Get(name)
	if !ok || entry.ID != id || entry.Phase != PhaseLive {
		t.Fatalf("entry = %+v", entry)
	}
}

func TestTheJITConfigGoesInTheInputAndNeverInAnArgv(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	if h.silo.inputs[h.silo.idOf(name)] != "JIT-SECRET-"+name {
		t.Fatalf("input = %q", h.silo.inputs[h.silo.idOf(name)])
	}
	for _, argv := range h.silo.argvs {
		if strings.Contains(strings.Join(argv, " "), "JIT") {
			t.Fatalf("the JIT configuration is in argv: %v", argv)
		}
	}
	for _, event := range h.log.all() {
		if strings.Contains(event, "JIT-SECRET") {
			t.Fatalf("the JIT configuration is in an event: %s", event)
		}
	}
}

func TestRedeliveredMessagesDoNotStartASecondRunner(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	h.scale(available(1))
	h.scale(nil)
	if n := h.log.count("fork "); n != 1 {
		t.Fatalf("forks = %d, events %v", n, h.log.all())
	}
	// A second job needs a second runner; the most is maxRunners.
	h.scale(available(1, 2))
	h.scale(available(1, 2, 3, 4, 5))
	if n := h.log.count("fork "); n != 3 {
		t.Fatalf("forks = %d, want 3 (maxRunners)", n)
	}
}

func TestASuccessfulJobDeregistersThenDeletesTheComputer(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	id := h.actions.idOf(name)
	h.scale(started(1, name))
	if h.status()[name] != "busy" {
		t.Fatalf("runners = %v", h.status())
	}
	h.scale(completed(1, name, "succeeded", 0))
	dereg, drop := h.log.index(fmt.Sprintf("deregister %d", id)), h.log.index("drop "+name)
	if dereg < 0 || drop < 0 || dereg > drop {
		t.Fatalf("deregister at %d, drop at %d: %v", dereg, drop, h.log.all())
	}
	if _, ok := h.silo.state(name); ok {
		t.Fatal("the computer is still there")
	}
	if len(h.status()) != 0 || len(h.ledger.All()) != 0 {
		t.Fatalf("runners %v ledger %v", h.status(), h.ledger.All())
	}
	if h.log.count("stop ") != 0 {
		t.Fatal("a successful computer must not be stopped and kept")
	}
}

func TestAFailedJobStopsAndKeepsTheComputerUnderTheRunnersName(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	h.scale(completed(1, name, "failed", 0))
	if state, ok := h.silo.state(name); !ok || state != "stopped" {
		t.Fatalf("state = %q, present %v", state, ok)
	}
	entry, _ := h.ledger.Get(name)
	if entry.Phase != PhaseKept || entry.Result != "failed" || h.log.count("drop ") != 0 {
		t.Fatalf("entry %+v events %v", entry, h.log.all())
	}
	if h.log.index("deregister") > h.log.index("stop "+name) {
		t.Fatalf("the runner must be deregistered before the computer is stopped: %v", h.log.all())
	}
	if h.log.count("checkpoint") != 0 {
		t.Fatal("no checkpoint may be taken after a job ran")
	}
}

func TestACancelledOrSkippedJobIsDeletedAndAnythingElseIsKept(t *testing.T) {
	for result, keep := range map[string]bool{"succeeded": false, "canceled": false, "skipped": false, "failed": true, "abandoned": true, "": true, resultUnknown: true,
		resultIdle: false, resultOrphan: false, resultShutdown: false, resultNotStarted: false, resultExited: false} {
		if keepResult(result) != keep {
			t.Errorf("keepResult(%q) = %v", result, !keep)
		}
	}
}

func TestKeepFailedCountDeletesTheOldestKeptComputers(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	var names []string
	for request := int64(1); request <= 3; request++ {
		h.advance(time.Minute)
		h.scale(available(request))
		name := h.only("idle")
		names = append(names, name)
		h.scale(started(request, name))
		h.scale(completed(request, name, "failed", 0))
	}
	// Count is 2: the first failure is gone, the other two stay.
	if _, ok := h.silo.state(names[0]); ok {
		t.Fatalf("the oldest kept computer is still there: %v", h.silo.names())
	}
	for _, name := range names[1:] {
		if state, ok := h.silo.state(name); !ok || state != "stopped" {
			t.Fatalf("%s: %q %v", name, state, ok)
		}
	}
	if got := len(h.ledger.All()); got != 2 {
		t.Fatalf("ledger = %v", h.ledger.All())
	}
}

func TestKeepFailedDaysDeletesOldKeptComputers(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(completed(1, name, "failed", 0))
	if h.status()[name] != "kept" {
		t.Fatalf("not kept: %v", h.status())
	}
	h.later(8 * 24 * time.Hour)
	if _, ok := h.silo.state(name); ok || len(h.ledger.All()) != 0 {
		t.Fatal("a computer kept for more than keepFailed.days must be deleted")
	}
}

func TestARunnerWhoseJobIsStillRunningIsNotRemoved(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	runnerID := h.actions.idOf(name)
	h.actions.removeErr[runnerID] = scaleset.JobStillRunningError
	h.scale(completed(1, name, "succeeded", 1))
	if state, ok := h.silo.state(name); !ok || state != "running" {
		t.Fatalf("the computer must keep running: %q %v", state, ok)
	}
	if h.log.count("drop ") != 0 || h.status()[name] != "busy" {
		t.Fatalf("events %v runners %v", h.log.all(), h.status())
	}
	// When the job really ends, the computer is cleaned up as usual.
	delete(h.actions.removeErr, runnerID)
	h.scale(completed(1, name, "succeeded", 0))
	if _, ok := h.silo.state(name); ok {
		t.Fatal("the computer should be deleted once its job is over")
	}
}

func TestAFailedDeregistrationKeepsTheComputerAndRetries(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	id := h.actions.idOf(name)
	h.actions.removeErr[id] = errors.New("GitHub is down")
	h.scale(completed(1, name, "succeeded", 0))
	if _, ok := h.silo.state(name); !ok {
		t.Fatal("the computer was deleted before the runner was deregistered")
	}
	delete(h.actions.removeErr, id)
	h.tick() // too early: the failed step waits
	if _, ok := h.silo.state(name); !ok {
		t.Fatal("the retry came before its time")
	}
	h.later(time.Minute)
	if _, ok := h.silo.state(name); ok {
		t.Fatal("the retry did not delete the computer")
	}
}

func TestACapacityLimitDefersTheRunnerWithoutForking(t *testing.T) {
	h := newHarness(t, harnessOptions{spec: ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 2, KeepFailed: KeepFailed{Count: intp(1), Days: 1}}})
	h.silo.add(Computer{Name: "owner-mac-1", Kind: "macos", State: "running"})
	h.silo.add(Computer{Name: "silo-build-x", Kind: "macos", State: "starting"})
	h.scale(available(1))
	if h.log.count("fork ") != 0 || len(h.status()) != 0 {
		t.Fatalf("no fork may happen at the limit: %v", h.log.all())
	}
	h.silo.setState("owner-mac-1", "stopped")
	h.scale(nil)
	if h.log.count("fork ") != 1 {
		t.Fatalf("the fork should happen once room exists: %v", h.log.all())
	}
}

func TestTwoMacOSSlotsAreSharedByEveryRunningMacVMAndForksInProgress(t *testing.T) {
	h := newHarness(t, harnessOptions{spec: ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 2, KeepFailed: KeepFailed{Count: intp(1), Days: 1}}})
	h.silo.add(Computer{Name: "owner-mac-1", Kind: "macos", State: "running"})
	// One slot is free and two jobs wait: the second fork would be the third VM.
	h.scale(available(1, 2))
	if n := h.log.count("fork "); n != 1 {
		t.Fatalf("forks = %d: %v", n, h.log.all())
	}
}

func TestTheDiskGuardStopsNewComputers(t *testing.T) {
	h := newHarness(t, harnessOptions{freeDisk: 5})
	h.scale(available(1))
	if h.log.count("fork ") != 0 {
		t.Fatalf("forked with a nearly full disk: %v", h.log.all())
	}
}

func TestNothingIsForkedWhileTheComputersCannotBeListed(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.listErr = errors.New("engine is not running")
	h.scale(available(1))
	if h.log.count("fork ") != 0 {
		t.Fatalf("capacity cannot be promised without a list: %v", h.log.all())
	}
	h.silo.listErr = nil
	h.later(time.Minute)
	if h.log.count("fork ") != 1 {
		t.Fatalf("the tick should have tried again: %v", h.log.all())
	}
}

func TestARunnerThatNeverListensIsReplacedByANewComputer(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.output = func(n int) []string {
		if n == 0 {
			return []string{"Connected to GitHub"} // never says it is listening
		}
		return []string{"Listening for Jobs"}
	}
	h.scale(available(1))
	first, second := "ci-silo-linux-00000001", "ci-silo-linux-00000002"
	if got := h.status(); len(got) != 1 || got[second] != "idle" {
		t.Fatalf("runners = %v", got)
	}
	if _, ok := h.silo.state(first); ok {
		t.Fatal("the computer that never became ready must be deleted")
	}
	dereg, drop := h.log.index("deregister"), h.log.index("drop "+first)
	if dereg < 0 || dereg > drop {
		t.Fatalf("the unused runner must be deregistered before its computer is deleted: %v", h.log.all())
	}
}

func TestARunnerThatStaysSilentUntilTheReadyTimeoutIsReplaced(t *testing.T) {
	h := newHarness(t, harnessOptions{readyTimeout: 10 * time.Millisecond})
	h.silo.output = func(n int) []string {
		if n == 0 {
			return []string{"Connected to GitHub", holdOpen} // alive, but never listening
		}
		return []string{"Listening for Jobs"}
	}
	h.scale(available(1))
	if got := h.status(); len(got) != 1 || got["ci-silo-linux-00000002"] != "idle" {
		t.Fatalf("runners = %v", got)
	}
	if _, ok := h.silo.state("ci-silo-linux-00000001"); ok {
		t.Fatal("the computer whose runner never listened must be deleted")
	}
}

// The command of a runner that never listened is still running when the helper gives up on it.
// It must be drained and waited for like any other, or every failed start leaves a `silo exec`
// process that is never reaped.
func TestTheCommandOfARunnerThatNeverListenedIsReaped(t *testing.T) {
	h := newHarness(t, harnessOptions{readyTimeout: 10 * time.Millisecond})
	h.silo.output = func(n int) []string {
		if n == 0 {
			return []string{"Connected to GitHub", holdOpen}
		}
		return []string{"Listening for Jobs"}
	}
	h.scale(available(1))
	if _, ok := h.silo.state("ci-silo-linux-00000001"); ok {
		t.Fatal("the computer whose runner never listened must be deleted")
	}
	h.silo.mu.Lock()
	silent := h.silo.runs[0]
	h.silo.mu.Unlock()
	waitFor(t, "the silent runner's command to be reaped", silent.waited.Load)
	if got := h.status(); len(got) != 1 || got["ci-silo-linux-00000002"] != "idle" {
		t.Fatalf("the end of the old command must not touch the new runner: %v", got)
	}
}

func TestAfterTheLastAttemptTheFleetBacksOff(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.output = func(int) []string { return []string{"nothing useful"} }
	h.scale(available(1))
	if n := h.log.count("fork "); n != 3 {
		t.Fatalf("attempts = %d", n)
	}
	h.scale(nil)
	if n := h.log.count("fork "); n != 3 {
		t.Fatalf("a poll right after must not try again: %d", n)
	}
	h.advance(time.Hour)
	h.silo.output = func(int) []string { return []string{"Listening for Jobs"} }
	h.scale(nil)
	if got := h.status(); len(got) != 1 {
		t.Fatalf("runners after the wait = %v", got)
	}
}

func TestAFailedForkLeavesNothingBehind(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.failFork = true
	h.scale(available(1))
	if len(h.status()) != 0 || len(h.ledger.All()) != 0 || h.log.count("jit ") != 0 || h.log.count("drop ") != 0 {
		t.Fatalf("events %v", h.log.all())
	}
}

func TestIdleRunnersNoJobNeedsAreRemovedAfterTheIdleTimeout(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1, 2))
	if len(h.status()) != 2 {
		t.Fatalf("runners = %v", h.status())
	}
	// Both jobs were cancelled before they started.
	h.scale(&scaleset.RunnerScaleSetMessage{Statistics: &scaleset.RunnerScaleSetStatistic{}, JobCompletedMessages: []*scaleset.JobCompleted{
		{Result: "canceled", JobMessageBase: scaleset.JobMessageBase{RunnerRequestID: 1}},
		{Result: "canceled", JobMessageBase: scaleset.JobMessageBase{RunnerRequestID: 2}},
	}})
	if len(h.status()) != 2 {
		t.Fatal("idle runners must wait out the idle timeout first")
	}
	h.advance(2 * time.Minute)
	h.scale(nil)
	if len(h.status()) != 0 || len(h.silo.names()) != 0 {
		t.Fatalf("runners %v computers %v", h.status(), h.silo.names())
	}
}

func TestShutdownRemovesIdleRunnersAndLeavesBusyOnes(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1, 2))
	var busy, idle string
	for name := range h.status() {
		busy = name
	}
	for name := range h.status() {
		if name != busy {
			idle = name
		}
	}
	h.scale(started(1, busy))
	h.fleet.Shutdown()
	ledger, err := OpenLedger(h.stateDir)
	if err != nil {
		t.Fatal(err)
	}
	if entries := ledger.All(); len(entries) != 1 || entries[0].Name != busy || entries[0].Phase != PhaseLive {
		t.Fatalf("ledger = %+v", entries)
	}
	if state, ok := h.silo.state(busy); !ok || state != "running" {
		t.Fatalf("the busy computer must keep running: %q %v", state, ok)
	}
	if _, ok := h.silo.state(idle); ok {
		t.Fatal("the idle computer must be deleted")
	}
}

func TestACrashedIdleRunnerIsReplaced(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.exited(name)
	if _, ok := h.silo.state(name); ok {
		t.Fatal("the crashed runner's computer is still there")
	}
	if got := h.status(); len(got) != 1 || got["ci-silo-linux-00000002"] != "idle" {
		t.Fatalf("a replacement should be running: %v", got)
	}
}

func TestTheRunnerCommandEndingIsReportedByItsWatcher(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.silo.procs[h.silo.idOf(name)].Kill()
	waitFor(t, "the replacement runner", func() bool { return h.status()["ci-silo-linux-00000002"] == "idle" })
	h.fleet.Settle()
	if _, ok := h.silo.state(name); ok {
		t.Fatal("the computer of the runner that ended is still there")
	}
}

func TestAcquireFailureReturnsTheErrorSoTheMessageIsRedelivered(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scaler.SetAcquirer(failingAcquirer{})
	err := h.scaler.Scale(context.Background(), available(1))
	if err == nil {
		t.Fatal("want an error")
	}
	h.fleet.Settle()
	if h.log.count("fork ") != 0 {
		t.Fatal("nothing may start for a job that was not acquired")
	}
}

type failingAcquirer struct{}

func (failingAcquirer) AcquireJobs(context.Context, []int64) ([]int64, error) {
	return nil, errors.New("no")
}

// MARK: lifecycle of a runner from its reservation to its removal

// neverReady makes the runner program print nothing useful, so every start fails.
func neverReady(h *harness) {
	h.silo.output = func(int) []string { return []string{"nothing useful"} }
}

func TestAStartThatFailsWhileItsJobIsRunningLeavesTheComputerAlone(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	neverReady(h)
	first := "ci-silo-linux-00000001"
	h.actions.removeErr[101] = scaleset.JobStillRunningError
	h.scale(available(1))
	if state, ok := h.silo.state(first); !ok || state != "running" {
		t.Fatalf("a computer whose runner still has a job must keep running: %q %v (%v)", state, ok, h.log.all())
	}
	if h.log.index("drop "+first) >= 0 || h.status()[first] != "busy" {
		t.Fatalf("events %v runners %v", h.log.all(), h.status())
	}
}

func TestAStartThatCannotDeregisterKeepsItsComputerAndRetries(t *testing.T) {
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
	if _, ok := h.silo.state(first); !ok || h.log.index("drop "+first) >= 0 {
		t.Fatalf("the computer was deleted before its runner was deregistered: %v", h.log.all())
	}
	if h.status()[first] != "finishing" {
		t.Fatalf("runners = %v", h.status())
	}
	delete(h.actions.removeErr, 101)
	h.later(time.Minute)
	if _, ok := h.silo.state(first); ok || len(h.status()) != 1 {
		t.Fatalf("the retry should delete it: runners %v", h.status())
	}
}

func TestAStartThatFailsAfterTheRunnerIsGoneAtGitHubDeletesTheComputer(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	neverReady(h)
	h.actions.removeErr[101] = scaleset.RunnerNotFoundError
	h.scale(available(1))
	if _, ok := h.silo.state("ci-silo-linux-00000001"); ok {
		t.Fatal("a runner confirmed absent no longer protects its computer")
	}
}

func TestAFailedStartIsTrackedAndItsDeleteRetried(t *testing.T) {
	h := newHarness(t, harnessOptions{attempts: 1})
	neverReady(h)
	h.silo.dropErr = errors.New("busy")
	h.scale(available(1))
	first := "ci-silo-linux-00000001"
	if h.status()[first] != "finishing" {
		t.Fatalf("a computer that could not be deleted must stay tracked: %v", h.status())
	}
	if _, ok := h.ledger.Get(first); !ok {
		t.Fatal("and recorded")
	}
	h.silo.dropErr = nil
	h.silo.output = func(int) []string { return []string{"Listening for Jobs"} }
	h.later(time.Hour)
	if _, ok := h.silo.state(first); ok {
		t.Fatal("the retry did not delete it")
	}
	if _, ok := h.ledger.Get(first); ok {
		t.Fatal("the record outlived the computer")
	}
}

func TestTwoFailedMacStartsDoNotHoldBothSlotsOnce(t *testing.T) {
	h := newHarness(t, harnessOptions{attempts: 1, spec: ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 2, KeepFailed: KeepFailed{Count: intp(1), Days: 1}}})
	h.silo.output = func(n int) []string {
		if n < 2 {
			return []string{"nothing useful"}
		}
		return []string{"Listening for Jobs"}
	}
	h.silo.dropErr = errors.New("busy")
	h.scale(available(1, 2))
	for name, state := range h.status() {
		if state != "finishing" {
			t.Fatalf("%s is %s: %v", name, state, h.status())
		}
	}
	if len(h.status()) != 2 {
		t.Fatalf("runners = %v", h.status())
	}
	h.silo.dropErr = nil
	h.later(time.Hour) // cleans up the failed computers and starts the replacements in the room that frees
	idle := 0
	for _, state := range h.status() {
		if state == "idle" {
			idle++
		}
	}
	if idle != 2 || len(h.status()) != 2 {
		t.Fatalf("both slots should be serving again: %v", h.status())
	}
}

func TestAJobThatEndsBeforeTheRunnerIsReadyKeepsItsResult(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.onRunner = func(id string) {
		name := h.silo.label(id)
		_ = h.scaler.Scale(context.Background(), started(1, name))
		_ = h.scaler.Scale(context.Background(), completed(1, name, "failed", 0))
	}
	h.scale(available(1))
	name := "ci-silo-linux-00000001"
	entry, _ := h.ledger.Get(name)
	if state, ok := h.silo.state(name); !ok || state != "stopped" || entry.Phase != PhaseKept || entry.Result != "failed" {
		t.Fatalf("a failed job's computer must be kept: %q %v entry %+v (%v)", state, ok, entry, h.log.all())
	}
	if h.log.index("drop ") >= 0 || len(h.status()) != 1 {
		t.Fatalf("events %v runners %v", h.log.all(), h.status())
	}
}

func TestAJobThatStartsBeforeTheRunnerIsReadyMakesItBusy(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.silo.onRunner = func(id string) { _ = h.scaler.Scale(context.Background(), started(1, h.silo.label(id))) }
	h.scale(available(1))
	if got := h.status()["ci-silo-linux-00000001"]; got != "busy" {
		t.Fatalf("state = %q", got)
	}
}

func TestABusyRunnerWhoseCommandEndedWithoutAResultIsKeptAsUnknown(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	// The runner program ends without GitHub ever reporting the job, and GitHub forgets the runner.
	h.exited(name)
	h.actions.forget(name)
	h.scale(&scaleset.RunnerScaleSetMessage{Statistics: &scaleset.RunnerScaleSetStatistic{}})
	h.later(time.Minute) // inside the grace period: still waiting for the result
	if h.status()[name] != "busy" {
		t.Fatalf("runners = %v", h.status())
	}
	h.later(3 * time.Minute)
	entry, _ := h.ledger.Get(name)
	if entry.Phase != PhaseKept || entry.Result != resultUnknown {
		t.Fatalf("a job of unknown result keeps its computer: %+v %v", entry, h.log.all())
	}
	var left int
	h.fleet.query(func() { left = len(h.fleet.sets["silo-linux"].outstanding) })
	if left != 0 {
		t.Fatalf("the settled request still asks for a runner: %d", left)
	}
	if n := h.log.count("fork "); n != 1 {
		t.Fatalf("a replacement was started for a settled job: %d forks", n)
	}
}

func TestAResultThatArrivesAfterTheCommandEndedIsUsed(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(started(1, name))
	h.exited(name)
	h.scale(completed(1, name, "succeeded", 0))
	if _, ok := h.silo.state(name); ok || len(h.ledger.All()) != 0 {
		t.Fatalf("a succeeded job's computer is deleted: %v", h.log.all())
	}
}

func TestAKeepIsRecordedBeforeTheRunnerIsDeregistered(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	h.scale(available(1))
	name := h.onlyRunner()
	h.actions.removeErr[h.actions.idOf(name)] = errors.New("GitHub is down")
	h.scale(completed(1, name, "failed", 0))
	entry, _ := h.ledger.Get(name)
	if entry.Phase != PhaseFinishing || entry.Result != "failed" {
		t.Fatalf("a crash now would leave a computer the next start drops: %+v", entry)
	}
	if state, _ := h.silo.state(name); state != "running" || h.log.count("drop ") != 0 {
		t.Fatalf("state %q events %v", state, h.log.all())
	}
}

func TestKeepFailedCountZeroDeletesAFailedComputerAtOnce(t *testing.T) {
	h := newHarness(t, harnessOptions{spec: ScaleSetSpec{Label: "silo-linux", Recipe: "silo-linux", OS: "linux", MaxRunners: 1, KeepFailed: KeepFailed{Count: intp(0), Days: 7}}})
	h.scale(available(1))
	name := h.onlyRunner()
	h.scale(completed(1, name, "failed", 0))
	if _, ok := h.silo.state(name); ok || len(h.ledger.All()) != 0 {
		t.Fatalf("nothing is kept with count 0: %v", h.silo.names())
	}
}

func TestTheFinishedSetStaysBounded(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	var size int
	var recent, old bool
	h.fleet.query(func() {
		set := h.fleet.sets["silo-linux"]
		for i := int64(1); i <= 10000; i++ {
			set.retire(i)
		}
		_, recent = set.finished[10000]
		_, old = set.finished[1]
		size = len(set.finished)
	})
	if size > 4096 || !recent || old {
		t.Fatalf("size %d recent %v old %v", size, recent, old)
	}
}

// After a Mac restarts its computers are stopped, but GitHub still lists their ephemeral runners,
// so the adopted records are busy. A list that shows the computer stopped means it holds no slot,
// and a job that waits gets a computer.
func TestAnAdoptedBusyRecordWhoseComputerIsStoppedDoesNotHoldAMacSlot(t *testing.T) {
	spec := ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 2, KeepFailed: KeepFailed{Count: intp(1), Days: 1}}
	h := newHarness(t, harnessOptions{spec: spec})
	name := "ci-silo-macos-aaaaaaaa"
	h.silo.add(Computer{ID: "id-" + name, Name: name, Kind: "macos", State: "stopped"})
	h.silo.add(Computer{Name: "owner-mac-1", Kind: "macos", State: "running"})
	h.actions.runners[name] = 50
	h.restart(Entry{Name: name, ID: "id-" + name, Label: "silo-macos", Phase: PhaseLive, Request: 5, At: h.clock()})
	h.scale(available(1, 2))
	if h.status()[name] != "busy" {
		t.Fatalf("the record is still adopted as busy: %v", h.status())
	}
	if n := h.log.count("fork "); n != 1 {
		t.Fatalf("the stopped computer must not hold a slot: forks = %d (%v)", n, h.log.all())
	}
}

// MARK: the private-repository check before each job

func TestAJobIsNotTakenWhenTheRepositoryIsNoLongerPrivate(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	public := errors.New("o/r is a public repository")
	h.scaler.check = func(context.Context) error { return public }
	acquirer := &fakeAcquirer{}
	h.scaler.SetAcquirer(acquirer)
	// Scale fails without acquiring, so the listener does not ack the message and GitHub
	// delivers it again; the check runs again before every attempt.
	if err := h.scaler.Scale(context.Background(), available(1)); !errors.Is(err, public) {
		t.Fatalf("err = %v", err)
	}
	if len(acquirer.calls) != 0 {
		t.Fatalf("the job was acquired: %v", acquirer.calls)
	}
	h.fleet.Settle()
	if len(h.status()) != 0 {
		t.Fatalf("a computer was started anyway: %v", h.status())
	}
	// A poll that brings no job does not run the check, so the listener keeps its session.
	if err := h.scaler.Scale(context.Background(), &scaleset.RunnerScaleSetMessage{Statistics: &scaleset.RunnerScaleSetStatistic{}}); err != nil {
		t.Fatalf("a statistics poll failed: %v", err)
	}
	// A check that cannot say (an outage, a rate limit) refuses the job the same way.
	h.scaler.check = func(context.Context) error { return errors.New("GitHub is down") }
	if err := h.scaler.Scale(context.Background(), available(2)); err == nil {
		t.Fatal("an unreadable check let the job through")
	}
	// A passing check lets the job through and a computer is forked for it.
	h.scaler.check = func(context.Context) error { return nil }
	h.scale(available(3))
	if len(acquirer.calls) != 1 || len(acquirer.calls[0]) != 1 || acquirer.calls[0][0] != 3 {
		t.Fatalf("the private repository's job was not acquired: %v", acquirer.calls)
	}
	if _, ok := h.silo.state("ci-silo-linux-00000001"); !ok {
		t.Fatalf("no computer for the job: %v", h.status())
	}
}

type dynamicChecker struct {
	mu  sync.Mutex
	err error
}

func (d *dynamicChecker) CheckPrivate(context.Context, Target, string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.err
}

func (d *dynamicChecker) set(err error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	d.err = err
}

func TestEventsDeliveredWhenPerJobPrivacyCheckFails(t *testing.T) {
	cfg := testConfig(t)
	checker := &dynamicChecker{}
	deps := &Deps{Checker: checker, Logger: quiet()}
	h := newHarness(t, harnessOptions{
		checkJobAccess: func(ctx context.Context) error { return CheckAccess(ctx, cfg, deps) },
	})
	acquirer := &fakeAcquirer{}
	h.scaler.SetAcquirer(acquirer)

	// A first job is taken while the repository is private.
	name := "ci-silo-linux-00000001"
	h.scale(available(1))
	if _, ok := h.silo.state(name); !ok {
		t.Fatalf("no computer started for the first job: %v", h.status())
	}
	h.scale(started(1, name))

	// The repository becomes public. A poll arrives bringing a new job offer together with
	// the completed event of the first job.
	public := errors.New("o/r is a public repository")
	checker.set(public)

	msg := &scaleset.RunnerScaleSetMessage{
		JobAvailableMessages: []*scaleset.JobAvailable{{RunnerRequestID: 2}},
		JobCompletedMessages: []*scaleset.JobCompleted{{RunnerRequestID: 1, RunnerName: name, Result: "succeeded"}},
		// GitHub still counts the offered job as assigned.
		Statistics: &scaleset.RunnerScaleSetStatistic{TotalAssignedJobs: 1},
	}
	err := h.scaler.Scale(context.Background(), msg)
	if !errors.Is(err, public) {
		t.Fatalf("Scale did not fail with the privacy check error: %v", err)
	}

	// The new job was not acquired (fail closed).
	if len(acquirer.calls) != 1 || len(acquirer.calls[0]) != 1 || acquirer.calls[0][0] != 1 {
		t.Fatalf("unexpected acquired jobs: %v", acquirer.calls)
	}

	// But the completed event was still delivered, so the finished job's computer was cleaned up.
	h.fleet.Settle()
	if _, ok := h.silo.state(name); ok {
		t.Fatalf("a succeeded computer was not deleted: %v", h.status())
	}
	// And no computer is made for the job that was not taken.
	if names := h.silo.names(); len(names) != 0 {
		t.Fatalf("a computer was made while the repository was public: %v", names)
	}
}
