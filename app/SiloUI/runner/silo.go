package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"strings"
	"sync"
	"time"
)

// Computer is one row of `silo list --all --json`.
type Computer struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Kind      string `json:"kind"`
	State     string `json:"state"`
	MemoryGiB int    `json:"memoryGiB"`
}

// Occupies reports whether the computer holds a virtual machine slot and its memory.
func (c Computer) Occupies() bool {
	switch c.State {
	case "running", "starting", "stopping":
		return true
	}
	return false
}

// Silo is everything the helper does to computers. The only implementation that matters runs
// the silo command line; tests use fakes.
type Silo interface {
	List(ctx context.Context) ([]Computer, error)
	// Fork makes a stopped computer called name from the cached build of recipe and returns its id.
	// It is the only call that goes by name; every later call goes by the computer's id.
	Fork(ctx context.Context, name, recipe string) (id string, err error)
	Start(ctx context.Context, id string) error
	Stop(ctx context.Context, id string) error
	// Drop deletes the computer, stopping it first when it is running.
	Drop(ctx context.Context, id string) error
	// StartRunner runs argv in the computer as its account. input is written to the command's
	// standard input and then closed; it never appears in a command line.
	StartRunner(ctx context.Context, id string, input string, argv []string) (Process, error)
	// BuildRecipe builds the recipe (a rebuild after its file changed).
	BuildRecipe(ctx context.Context, recipe string) error
}

// Process is a command running in a computer.
type Process interface {
	// Lines delivers the command's output, a line at a time, and closes when it ends.
	Lines() <-chan string
	// Wait returns the command's exit status once it has ended.
	Wait() error
	// Kill stops waiting for the command; the command itself ends with the computer.
	Kill()
}

// CLI drives silo through its command line.
type CLI struct {
	Path    string
	Dir     string
	Socket  string
	Device  string
	Timeout time.Duration
}

func (c *CLI) args(rest ...string) []string {
	var args []string
	if c.Socket != "" {
		args = append(args, "--socket", c.Socket)
	}
	if c.Device != "" {
		args = append(args, "--device", c.Device)
	}
	return append(args, rest...)
}

func (c *CLI) run(ctx context.Context, rest ...string) ([]byte, error) {
	if c.Timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, c.Timeout)
		defer cancel()
	}
	cmd := exec.CommandContext(ctx, c.Path, c.args(rest...)...)
	cmd.Dir = c.Dir
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	if err := cmd.Run(); err != nil {
		err = fmt.Errorf("silo %s: %w: %s", strings.Join(rest[:min(len(rest), 2)], " "), err, tail(stderr.String(), 400))
		if strings.Contains(stderr.String(), nameTakenMessage) {
			err = fmt.Errorf("%w: %w", errNameTaken, err)
		}
		return nil, err
	}
	return stdout.Bytes(), nil
}

// nameTakenMessage is what the engine answers when a computer name is already used
// (computer_names.rs TAKEN). The command line prints errors as text without a code, so this
// sentence is the only mark of the refusal.
const nameTakenMessage = "Computer names must be unique."

// errNameTaken marks a failure of the engine to make a computer because its name was already
// used: nothing was made, and the computer that has the name is someone else's.
var errNameTaken = errors.New("the computer name is already used")

func tail(text string, limit int) string {
	text = strings.TrimSpace(text)
	if len(text) > limit {
		return "..." + text[len(text)-limit:]
	}
	return text
}

// List returns every computer, hidden throwaways included: a recipe build is a virtual machine.
func (c *CLI) List(ctx context.Context) ([]Computer, error) {
	out, err := c.run(ctx, "list", "--all", "--json")
	if err != nil {
		return nil, err
	}
	var listed struct {
		Computers []Computer `json:"computers"`
	}
	if err := json.Unmarshal(out, &listed); err != nil {
		return nil, fmt.Errorf("silo list: unreadable output: %w", err)
	}
	return listed.Computers, nil
}

// Fork implements Silo. A refusal because the name is taken is an error that wraps errNameTaken.
func (c *CLI) Fork(ctx context.Context, name, recipe string) (string, error) {
	out, err := c.run(ctx, "fork", name, "--recipe", recipe, "--json")
	if err != nil {
		return "", err
	}
	// `silo fork <name> --recipe <recipe> --json` prints the new computer itself; forks of an
	// existing computer print {"forks": [...]}. Both carry the engine's id.
	var made struct {
		ID    string `json:"id"`
		Forks []struct {
			ID string `json:"id"`
		} `json:"forks"`
	}
	if err := json.Unmarshal(out, &made); err != nil {
		return "", fmt.Errorf("silo fork: no computer id in the output: %s", tail(string(out), 200))
	}
	if made.ID != "" {
		return made.ID, nil
	}
	if len(made.Forks) == 1 && made.Forks[0].ID != "" {
		return made.Forks[0].ID, nil
	}
	return "", fmt.Errorf("silo fork: no computer id in the output: %s", tail(string(out), 200))
}

// The silo command resolves its computer argument as a name first and then as an id
// (cli/src/engine.rs find). Ids are UUIDs and the helper's names are ci-<label>-<id>, so an id
// could only be shadowed by someone naming a computer with another computer's UUID.

// Start implements Silo.
func (c *CLI) Start(ctx context.Context, id string) error {
	_, err := c.run(ctx, "start", id, "--json")
	return err
}

// Stop implements Silo. The engine refuses to stop a computer that is not running, so one a list
// shows as stopped is already done: a stop that was answered but not recorded is not retried
// into an error forever.
func (c *CLI) Stop(ctx context.Context, id string) error {
	computers, err := c.List(ctx)
	if err != nil {
		return err
	}
	for _, computer := range computers {
		if computer.ID == id && computer.State == "stopped" {
			return nil
		}
	}
	return c.stop(ctx, id)
}

func (c *CLI) stop(ctx context.Context, id string) error {
	_, err := c.run(ctx, "stop", id, "--json")
	return err
}

// Drop implements Silo. `silo drop` does not stop a running computer, and the engine refuses to
// delete one, so a computer that holds a virtual machine is stopped first. A computer that is not
// in the list is an error: only a fresh list may say a computer is gone.
func (c *CLI) Drop(ctx context.Context, id string) error {
	computers, err := c.List(ctx)
	if err != nil {
		return err
	}
	var found *Computer
	for i := range computers {
		if computers[i].ID == id {
			found = &computers[i]
		}
	}
	if found == nil {
		return fmt.Errorf("silo drop: no computer has the id %s", id)
	}
	if found.Occupies() {
		if err := c.stop(ctx, id); err != nil {
			return err
		}
	}
	_, err = c.run(ctx, "drop", id, "--json")
	return err
}

// BuildRecipe implements Silo. A build takes minutes, so it has no timeout of its own.
func (c *CLI) BuildRecipe(ctx context.Context, recipe string) error {
	long := *c
	long.Timeout = 0
	_, err := long.run(ctx, "recipe", "build", recipe, "--json")
	return err
}

// StartRunner implements Silo with `silo exec --stream --stdin`: no time limit, output as it
// arrives, and the input in the request to the engine instead of a command line.
func (c *CLI) StartRunner(ctx context.Context, id, input string, argv []string) (Process, error) {
	args := c.args(append([]string{"exec", id, "--stream", "--stdin", "--"}, argv...)...)
	cmd := exec.CommandContext(ctx, c.Path, args...)
	cmd.Dir = c.Dir
	cmd.WaitDelay = 5 * time.Second
	cmd.Cancel = func() error { return cmd.Process.Signal(interruptSignal) }
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("silo exec: %w", err)
	}
	proc := &cliProcess{cmd: cmd, lines: make(chan string, 64)}
	var readers sync.WaitGroup
	for _, stream := range []io.Reader{stdout, stderr} {
		readers.Add(1)
		go func() {
			defer readers.Done()
			scanner := bufio.NewScanner(stream)
			scanner.Buffer(make([]byte, 64<<10), 1<<20)
			for scanner.Scan() {
				proc.lines <- scanner.Text()
			}
		}()
	}
	go func() {
		readers.Wait()
		close(proc.lines)
	}()
	go func() {
		_, _ = io.WriteString(stdin, input+"\n")
		_ = stdin.Close()
	}()
	return proc, nil
}

type cliProcess struct {
	cmd   *exec.Cmd
	lines chan string
	once  sync.Once
	err   error
}

func (p *cliProcess) Lines() <-chan string { return p.lines }

func (p *cliProcess) Wait() error {
	p.once.Do(func() { p.err = p.cmd.Wait() })
	return p.err
}

func (p *cliProcess) Kill() {
	if p.cmd.Process != nil {
		_ = p.cmd.Process.Signal(interruptSignal)
	}
}

// errNoCapacity means the host cannot take another computer now.
var errNoCapacity = errors.New("no capacity")
