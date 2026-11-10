package main

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	"github.com/actions/scaleset"
)

// readyLine is what GitHub's runner prints once it is connected and waiting for a job.
const readyLine = "Listening for Jobs"

// Actions is the part of the scaleset client the fleet uses.
type Actions interface {
	GenerateJitRunnerConfig(ctx context.Context, setting *scaleset.RunnerScaleSetJitRunnerSetting, scaleSetID int) (*scaleset.RunnerScaleSetJitRunnerConfig, error)
	GetRunnerByName(ctx context.Context, name string) (*scaleset.RunnerReference, error)
	RemoveRunner(ctx context.Context, runnerID int64) error
}

// Acquirer takes jobs from the scale set's message session.
type Acquirer interface {
	AcquireJobs(ctx context.Context, requestIDs []int64) ([]int64, error)
}

// Scaler is the listener's view of one scale set. It implements listener.Scaler: it acquires the
// jobs a message offers and hands the message to the fleet's event loop, which does the rest.
type Scaler struct {
	fleet    *Fleet
	label    string
	acquirer atomic.Pointer[Acquirer]
	beat     atomic.Int64
	// check refuses a poll that brings jobs when the repository may not be served any more (the
	// private-repository check, run again before each job). An error leaves the message unacked,
	// so GitHub offers the job again and the check runs again.
	check func(context.Context) error
}

// SetAcquirer sets the message session that jobs are acquired from; a new session replaces it.
func (s *Scaler) SetAcquirer(a Acquirer) {
	s.acquirer.Store(&a)
	s.beat.Store(time.Now().UnixNano())
}

// LastScale is when the listener last called Scale.
func (s *Scaler) LastScale() time.Time { return time.Unix(0, s.beat.Load()) }

// Scale handles one poll of the scale set. It returns once the event loop has taken the message in.
func (s *Scaler) Scale(ctx context.Context, msg *scaleset.RunnerScaleSetMessage) error {
	s.beat.Store(time.Now().UnixNano())
	event := messageEvent{label: s.label, reply: make(chan struct{})}
	var checkErr error
	if msg != nil {
		if len(msg.JobAvailableMessages) > 0 {
			// Taking a job runs code on this machine, so the repository is checked again first, as
			// at registration. A public repository, or one the check cannot read, is not served:
			// no job is acquired, but started and completed events from the poll are still delivered.
			if s.check != nil {
				if err := s.check(ctx); err != nil {
					checkErr = fmt.Errorf("the job is not taken: %w", err)
				}
			}
			if checkErr == nil {
				ids := make([]int64, 0, len(msg.JobAvailableMessages))
				for _, job := range msg.JobAvailableMessages {
					ids = append(ids, job.RunnerRequestID)
				}
				acquirer := s.acquirer.Load()
				if acquirer == nil {
					return errors.New("no message session to acquire jobs from")
				}
				acquired, err := (*acquirer).AcquireJobs(ctx, ids)
				if err != nil {
					return fmt.Errorf("cannot acquire jobs: %w", err)
				}
				event.acquired = acquired
			}
		}
		event.started, event.completed = msg.JobStartedMessages, msg.JobCompletedMessages
		if checkErr != nil {
			// No runner is asked for while the repository is not served; the jobs in flight
			// keep theirs.
			none := 0
			event.assigned = &none
		} else if msg.Statistics != nil {
			assigned := msg.Statistics.TotalAssignedJobs
			event.assigned = &assigned
		}
	}
	if err := s.fleet.deliver(event); err != nil {
		return err
	}
	return checkErr
}

// ComputerName is the name of a computer and of its runner: ci-<label>-<short id>.
func ComputerName(label, id string) string { return "ci-" + label + "-" + id }

// The helper's own reasons for ending a runner. A job's result comes from GitHub; these never do.
const (
	resultOrphan     = "orphan"
	resultIdle       = "idle"
	resultShutdown   = "shutdown"
	resultNotStarted = "the runner did not start"
	resultExited     = "runner exited while idle"
	// resultUnknown: the job ended while the helper was not watching (or its runner command
	// ended without a result), so how it ended is not known.
	resultUnknown = "unknown"
)

func isOwnReason(result string) bool {
	switch result {
	case resultOrphan, resultIdle, resultShutdown, resultNotStarted, resultExited, resultUnknown:
		return true
	}
	return false
}

// keepResult says whether a computer is kept for a result. A computer is thrown away after a
// success, a cancellation or a skip, and after the helper's own reasons for removing a runner that
// had no job; it is kept after anything else, including a result nobody knows.
func keepResult(result string) bool {
	switch strings.ToLower(result) {
	case "succeeded", "canceled", "cancelled", "skipped":
		return false
	case resultOrphan, resultIdle, resultShutdown, resultNotStarted, resultExited:
		return false
	}
	return true
}

// retire ends a job request on the set: it needs no runner any more and is not taken again. The
// last 4096 are remembered, so a redelivered JobAvailable is not taken twice.
func (s *setState) retire(request int64) {
	if request == 0 {
		return
	}
	delete(s.outstanding, request)
	if _, seen := s.finished[request]; seen {
		return
	}
	s.finished[request] = struct{}{}
	s.finishedOrder = append(s.finishedOrder, request)
	if len(s.finishedOrder) > 4096 {
		for _, old := range s.finishedOrder[:2048] {
			delete(s.finished, old)
		}
		s.finishedOrder = append([]int64(nil), s.finishedOrder[2048:]...)
	}
}

// failed counts a computer whose runner did not come up. After startAttempts in a row the set
// waits longer before the next poll tries again, up to 16 minutes.
func (s *setState) failed(now time.Time, attempts int) (wait time.Duration) {
	s.attempts++
	if s.attempts < attempts {
		return 0
	}
	s.attempts = 0
	s.failStreak++
	wait = min(time.Duration(1<<min(s.failStreak, 5))*30*time.Second, 30*time.Minute)
	s.backoffTill = now.Add(wait)
	return wait
}

// succeeded clears the failures after a runner came up.
func (s *setState) succeeded() { s.attempts, s.failStreak = 0, 0 }

// waitReady waits for the runner to say it is listening for jobs.
func waitReady(ctx context.Context, proc Process, timeout time.Duration) error {
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	var last []string
	for {
		select {
		case line, ok := <-proc.Lines():
			if !ok {
				return fmt.Errorf("the runner ended before it was ready: %v: %s", proc.Wait(), strings.Join(last, " | "))
			}
			if strings.Contains(line, readyLine) {
				return nil
			}
			last = append(last, line)
			if len(last) > 5 {
				last = last[1:]
			}
		case <-timer.C:
			return fmt.Errorf("the runner was not ready after %s: %s", timeout, strings.Join(last, " | "))
		case <-ctx.Done():
			return ctx.Err()
		}
	}
}
