package main

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"os/user"
	"path/filepath"
	"regexp"
	"strings"
)

// serviceName is the systemd user unit of the helper. One helper runs per machine.
const serviceName = "silo-runner.service"

// defaultEngineUnit is the production engine's unit, the one `silod --service register` writes.
// A development engine's is org.silo.dev.engine.service.
const defaultEngineUnit = "org.silo.preview.engine.service"

// serviceMarker is the first line of the unit file the helper writes. A file at the unit's path
// without it is someone else's, and the helper never replaces or removes it.
const serviceMarker = "# Written by `silo-runner service register`. silo-runner rewrites and removes this file."

var unitNamePattern = regexp.MustCompile(`^[A-Za-z0-9:_.@-]+\.service$`)

// Systemd runs systemctl and loginctl for the account's user manager. Tests replace it.
type Systemd interface {
	Run(program string, args ...string) (string, error)
}

type realSystemd struct{}

func (realSystemd) Run(program string, args ...string) (string, error) {
	var out bytes.Buffer
	cmd := exec.Command(program, args...)
	cmd.Stdout, cmd.Stderr = &out, &out
	err := cmd.Run()
	return strings.TrimSpace(out.String()), err
}

// ServiceUnit is the user unit text. It starts the helper after the engine's unit, wants that
// unit so the engine comes up first when it is not running, and restarts the helper after a
// failure. It has no User=: a user unit runs as the account that owns the user manager, which is
// the account of the engine's socket.
//
// siloPath is the absolute path of the silo command (see ResolveSilo). The user manager's PATH
// lacks ~/.local/bin, where `silo host init` links silo, so a bare "silo" would fail every call and
// the unit would restart every ten seconds for ever.
func ServiceUnit(program, configPath, siloPath, engineUnit string) (string, error) {
	if !unitNamePattern.MatchString(engineUnit) {
		return "", fmt.Errorf("%q is not a systemd service name such as %s", engineUnit, defaultEngineUnit)
	}
	exe, err := execArgument(program)
	if err != nil {
		return "", err
	}
	config, err := execArgument(configPath)
	if err != nil {
		return "", err
	}
	silo, err := execArgument(siloPath)
	if err != nil {
		return "", fmt.Errorf("silo.path: %w", err)
	}
	return fmt.Sprintf(`%s
[Unit]
Description=Silo CI runner (silo-runner)
Documentation=https://github.com/amontlabs/silo/blob/main/docs/SiloUI-CI-RUNNER.md#linux-hosts
# Start after the engine whose socket the helper drives, and bring the engine up if it is down.
After=%[2]s
Wants=%[2]s
# A failed helper is restarted however often it fails, never left stopped by a rate limit.
StartLimitIntervalSec=0

[Service]
Type=simple
ExecStart=%[3]s run -config %[4]s -silo %[5]s
# The GitHub token, when the configuration reads one (github.auth.tokenEnv), goes in this
# optional file, mode 0600: SILO_RUNNER_TOKEN=...
EnvironmentFile=-%%E/silo-runner/env
Restart=on-failure
RestartSec=10
UMask=0077
# SIGTERM starts the helper's own shutdown: it removes idle runners and the computers it was
# still starting, and leaves computers with a job running for the next start. Only the helper
# is signalled first; whatever is left after the timeout is killed.
KillMode=mixed
KillSignal=SIGTERM
TimeoutStopSec=120

[Install]
WantedBy=default.target
`, serviceMarker, engineUnit, exe, config, silo), nil
}

// execArgument quotes path as one ExecStart word, escaping what systemd would expand.
func execArgument(path string) (string, error) {
	if !filepath.IsAbs(path) || strings.ContainsFunc(path, func(r rune) bool { return r < ' ' || r == 0x7f }) {
		return "", fmt.Errorf("%q cannot start a systemd unit: it must be an absolute path without control characters", path)
	}
	var quoted strings.Builder
	quoted.WriteByte('"')
	for _, r := range path {
		switch r {
		case '\\':
			quoted.WriteString(`\\`)
		case '"':
			quoted.WriteString(`\"`)
		case '%':
			quoted.WriteString("%%")
		case '$':
			quoted.WriteString("$$")
		default:
			quoted.WriteRune(r)
		}
	}
	quoted.WriteByte('"')
	return quoted.String(), nil
}

// ServiceSetup is everything register and unregister need.
type ServiceSetup struct {
	// UnitDir is the user unit directory, $XDG_CONFIG_HOME/systemd/user.
	UnitDir string
	Systemd Systemd
	Out     io.Writer
	// Account is the name `loginctl enable-linger` takes.
	Account string
	Euid    int
}

// DefaultServiceSetup is the running account's.
func DefaultServiceSetup(out io.Writer) (ServiceSetup, error) {
	config, err := userConfigDir()
	if err != nil {
		return ServiceSetup{}, err
	}
	current, err := user.Current()
	if err != nil {
		return ServiceSetup{}, fmt.Errorf("cannot look up the account: %w", err)
	}
	return ServiceSetup{UnitDir: filepath.Join(config, "systemd", "user"), Systemd: realSystemd{}, Out: out, Account: current.Username, Euid: os.Geteuid()}, nil
}

// userConfigDir is $XDG_CONFIG_HOME, or ~/.config when it is not set to an absolute path: the
// systemd specifier %E of a user unit.
func userConfigDir() (string, error) {
	config := os.Getenv("XDG_CONFIG_HOME")
	if filepath.IsAbs(config) {
		return config, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("cannot find the home directory: %w", err)
	}
	return filepath.Join(home, ".config"), nil
}

func (s ServiceSetup) unitPath() string { return filepath.Join(s.UnitDir, serviceName) }

func (s ServiceSetup) systemctl(args ...string) error {
	out, err := s.Systemd.Run("systemctl", append([]string{"--user"}, args...)...)
	if err != nil {
		return fmt.Errorf("systemctl --user %s failed: %s%w (systemctl --user needs the account's user manager: run this from a login session, or set XDG_RUNTIME_DIR=/run/user/$(id -u))",
			strings.Join(args, " "), prefix(out), err)
	}
	return nil
}

func prefix(out string) string {
	if out == "" {
		return ""
	}
	return out + ": "
}

// readUnit returns the current unit file's text, or "" when there is none. It refuses a file
// the helper did not write.
func (s ServiceSetup) readUnit() (string, error) {
	path := s.unitPath()
	info, err := os.Lstat(path)
	switch {
	case errors.Is(err, os.ErrNotExist):
		return "", nil
	case err != nil:
		return "", err
	case !info.Mode().IsRegular() || info.Size() > 64<<10:
		return "", fmt.Errorf("%s is not a unit file silo-runner wrote; it is left alone", path)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	if !strings.HasPrefix(string(data), serviceMarker+"\n") {
		return "", fmt.Errorf("%s was not written by silo-runner; it is left alone (remove it or use another account)", path)
	}
	return string(data), nil
}

// ResolveSilo returns the absolute path of the silo command that silo.path names. A bare name is
// looked up on PATH of the shell that registers the service; a path must be absolute and must be
// an executable file. The service runs with the user manager's PATH, which usually lacks
// ~/.local/bin, so it is given this path and never searches.
func ResolveSilo(path string, lookPath func(string) (string, error)) (string, error) {
	hint := "set silo.path to the absolute path of the silo command (`silo host init` links it at ~/.local/bin/silo, or silo-dev for a Silo Dev engine)"
	if path == "" {
		return "", errors.New("silo.path is empty: " + hint)
	}
	found := path
	if strings.Contains(path, "/") {
		if !filepath.IsAbs(path) {
			return "", fmt.Errorf("silo.path %q is relative to a directory the service does not start in: %s", path, hint)
		}
	} else {
		looked, err := lookPath(path)
		if err != nil {
			return "", fmt.Errorf("cannot find %q on PATH, and the service would not find it either: %s", path, hint)
		}
		if found, err = filepath.Abs(looked); err != nil {
			return "", err
		}
	}
	info, err := os.Stat(found)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0o111 == 0 {
		return "", fmt.Errorf("%s is not an executable file: %s", found, hint)
	}
	return found, nil
}

// RegisterService writes the unit and starts it. program is the silo-runner to run and
// configPath the configuration it reads; both are made absolute with links resolved. siloPath is
// the absolute silo command the unit passes to the helper (-silo).
func RegisterService(s ServiceSetup, program, configPath, siloPath, engineUnit string) error {
	if s.Euid == 0 {
		return errors.New("silo-runner does not run as root: register the service as the account that runs silod")
	}
	program, err := canonical(program)
	if err != nil {
		return fmt.Errorf("cannot find silo-runner: %w", err)
	}
	configPath, err = canonical(configPath)
	if err != nil {
		return fmt.Errorf("cannot find the configuration: %w", err)
	}
	unit, err := ServiceUnit(program, configPath, siloPath, engineUnit)
	if err != nil {
		return err
	}
	if _, err := s.readUnit(); err != nil {
		return err
	}
	if err := os.MkdirAll(s.UnitDir, 0o755); err != nil {
		return err
	}
	if err := writeAtomic(s.unitPath(), unit); err != nil {
		return err
	}
	for _, args := range [][]string{{"daemon-reload"}, {"enable", serviceName}, {"restart", serviceName}} {
		if err := s.systemctl(args...); err != nil {
			return err
		}
	}
	fmt.Fprintf(s.Out, "registered %s and started it, after %s\n", s.unitPath(), engineUnit)
	fmt.Fprintf(s.Out, "logs: journalctl --user -u %s\n", serviceName)
	s.reportLinger()
	return nil
}

// UnregisterService stops and removes the unit.
func UnregisterService(s ServiceSetup) error {
	text, err := s.readUnit()
	if err != nil {
		return err
	}
	if text == "" {
		fmt.Fprintf(s.Out, "%s is not registered\n", serviceName)
		return nil
	}
	if err := s.systemctl("disable", "--now", serviceName); err != nil {
		return err
	}
	if err := os.Remove(s.unitPath()); err != nil {
		return err
	}
	if err := s.systemctl("daemon-reload"); err != nil {
		return err
	}
	fmt.Fprintf(s.Out, "removed %s\n", s.unitPath())
	return nil
}

// reportLinger says how to keep the service running without a login session.
func (s ServiceSetup) reportLinger() {
	out, err := s.Systemd.Run("loginctl", "show-user", s.Account, "--property=Linger", "--value")
	if err == nil && out == "yes" {
		fmt.Fprintln(s.Out, "linger: yes")
		return
	}
	fmt.Fprintf(s.Out, "linger: no. Without it the service stops at the account's last logout and starts only at its first login. An administrator turns it on:\n  sudo loginctl enable-linger %s\n", s.Account)
}

// canonical makes path absolute and resolves links.
func canonical(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(abs)
}
