package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/actions/scaleset"
	"github.com/actions/scaleset/listener"
)

// Session is a message session of a scale set.
type Session interface {
	listener.Client
	Close(ctx context.Context) error
}

// Runnable is a listener.
type Runnable interface {
	Run(ctx context.Context, scaler listener.Scaler) error
}

var errStalled = errors.New("the listener stopped polling")

// ListenerLoop keeps a scale set's listener running. If no poll returns within Stall it ends the
// session and opens a new one; any other end of the listener starts a new session too.
type ListenerLoop struct {
	Scaler      *Scaler
	OpenSession func(ctx context.Context) (Session, error)
	NewListener func(Session) (Runnable, error)
	Stall       time.Duration
	// Check is how often the watchdog looks; Backoff is the pause after a failed session.
	Check   time.Duration
	Backoff time.Duration
	Logger  *slog.Logger
	// Restarts counts sessions that ended because of a stall.
	Restarts int
}

// Run returns when ctx ends.
func (l *ListenerLoop) Run(ctx context.Context) error {
	if l.Logger == nil {
		l.Logger = slog.New(slog.DiscardHandler)
	}
	if l.Check == 0 {
		l.Check = l.Stall / 4
	}
	if l.Backoff == 0 {
		l.Backoff = 5 * time.Second
	}
	for ctx.Err() == nil {
		err := l.once(ctx)
		if ctx.Err() != nil {
			return ctx.Err()
		}
		l.Logger.Warn("the listener ended; opening a new session", "error", fmt.Sprint(err))
		select {
		case <-time.After(l.Backoff):
		case <-ctx.Done():
		}
	}
	return ctx.Err()
}

func (l *ListenerLoop) once(ctx context.Context) error {
	session, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	sess, err := l.OpenSession(session)
	if err != nil {
		return fmt.Errorf("cannot open a message session: %w", err)
	}
	defer sess.Close(context.WithoutCancel(ctx))
	lst, err := l.NewListener(sess)
	if err != nil {
		return err
	}
	l.Scaler.SetAcquirer(sess)
	stop := make(chan struct{})
	defer close(stop)
	go func() {
		ticker := time.NewTicker(l.Check)
		defer ticker.Stop()
		for {
			select {
			case <-stop:
				return
			case <-ticker.C:
				if time.Since(l.Scaler.LastScale()) > l.Stall {
					cancel(errStalled)
					return
				}
			}
		}
	}()
	err = lst.Run(session, l.Scaler)
	if errors.Is(context.Cause(session), errStalled) {
		l.Restarts++
		return errStalled
	}
	return err
}

// sessionAdapter opens real message sessions.
type scaleAPI struct {
	*scaleset.Client
}

// OpenSession opens a message session for a scale set.
func (a scaleAPI) OpenSession(ctx context.Context, scaleSetID int, owner string) (Session, error) {
	return a.Client.MessageSessionClient(ctx, scaleSetID, owner)
}
