package main

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

type recordedSystemd struct {
	calls  []string
	fail   map[string]error
	linger string
}

func (r *recordedSystemd) Run(program string, args ...string) (string, error) {
	call := program + " " + strings.Join(args, " ")
	r.calls = append(r.calls, call)
	for prefix, err := range r.fail {
		if strings.HasPrefix(call, prefix) {
			return "boom", err
		}
	}
	if program == "loginctl" {
		return r.linger, nil
	}
	return "", nil
}

func serviceFixture(t *testing.T) (ServiceSetup, *recordedSystemd, *strings.Builder, string, string) {
	t.Helper()
	dir := t.TempDir()
	program := filepath.Join(dir, "silo-runner")
	config := filepath.Join(dir, "config.yaml")
	for _, path := range []string{program, config} {
		if err := os.WriteFile(path, []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	// The unit records the resolved path (t.TempDir can sit behind a link on a Mac).
	program, _ = filepath.EvalSymlinks(program)
	config, _ = filepath.EvalSymlinks(config)
	systemd := &recordedSystemd{linger: "no"}
	var out strings.Builder
	return ServiceSetup{UnitDir: filepath.Join(dir, "systemd", "user"), Systemd: systemd, Out: &out, Account: "ci", Euid: 1000}, systemd, &out, program, config
}

const silo = "/home/ci/.local/bin/silo"

func TestTheUnitStartsTheHelperAfterTheEngineAsTheAccount(t *testing.T) {
	unit, err := ServiceUnit("/home/ci/.local/bin/silo-runner", "/home/ci/runner.yaml", "/home/ci/.local/bin/silo", defaultEngineUnit)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{
		serviceMarker + "\n[Unit]",
		"After=org.silo.preview.engine.service\n",
		"Wants=org.silo.preview.engine.service\n",
		`ExecStart="/home/ci/.local/bin/silo-runner" run -config "/home/ci/runner.yaml" -silo "/home/ci/.local/bin/silo"`,
		"Restart=on-failure\n",
		"StartLimitIntervalSec=0\n",
		"UMask=0077\n",
		"KillSignal=SIGTERM\n",
		"EnvironmentFile=-%E/silo-runner/env\n",
		"WantedBy=default.target\n",
	} {
		if !strings.Contains(unit, want) {
			t.Fatalf("the unit lacks %q:\n%s", want, unit)
		}
	}
	// A user unit already runs as the account of the user manager; User= is for system units, and
	// a unit that named root would be wrong.
	for _, banned := range []string{"User=", "Group=", "WantedBy=multi-user.target"} {
		if strings.Contains(unit, banned) {
			t.Fatalf("the unit has %q", banned)
		}
	}
	// StartLimitIntervalSec belongs in [Unit], where systemd reads it.
	if !strings.Contains(strings.SplitN(unit, "[Service]", 2)[0], "StartLimitIntervalSec=0") {
		t.Fatal("StartLimitIntervalSec is not in [Unit]")
	}
}

func TestTheUnitCanFollowADevelopmentEngine(t *testing.T) {
	unit, err := ServiceUnit("/a/silo-runner", "/a/c.yaml", "/a/silo", "org.silo.dev.engine.service")
	if err != nil || !strings.Contains(unit, "After=org.silo.dev.engine.service\n") {
		t.Fatalf("err %v\n%s", err, unit)
	}
	for _, bad := range []string{"", "engine", "a b.service", "x.service\nExecStart=/bin/sh", "../x.service"} {
		if _, err := ServiceUnit("/a/silo-runner", "/a/c.yaml", "/a/silo", bad); err == nil {
			t.Fatalf("accepted engine unit %q", bad)
		}
	}
}

func TestPathsAreQuotedSoSystemdExpandsNothing(t *testing.T) {
	unit, err := ServiceUnit(`/home/a b/100%/$HOME/"q"\x`, "/c.yaml", "/s", defaultEngineUnit)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(unit, `ExecStart="/home/a b/100%%/$$HOME/\"q\"\\x" run`) {
		t.Fatal(unit)
	}
	for _, bad := range []string{"relative/silo-runner", "/a\nb", ""} {
		if _, err := ServiceUnit(bad, "/c.yaml", "/s", defaultEngineUnit); err == nil {
			t.Fatalf("accepted program %q", bad)
		}
		if _, err := ServiceUnit("/p", bad, "/s", defaultEngineUnit); err == nil {
			t.Fatalf("accepted config %q", bad)
		}
	}
}

func TestRegisterWritesTheUnitThenReloadsEnablesAndStartsIt(t *testing.T) {
	setup, systemd, out, program, config := serviceFixture(t)
	if err := RegisterService(setup, program, config, silo, defaultEngineUnit); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(setup.unitPath())
	if err != nil || !strings.Contains(string(data), `ExecStart="`+program+`" run -config "`+config+`" -silo "`+silo+`"`) {
		t.Fatalf("err %v unit %s", err, data)
	}
	want := []string{
		"systemctl --user daemon-reload", "systemctl --user enable silo-runner.service", "systemctl --user restart silo-runner.service",
		"loginctl show-user ci --property=Linger --value",
	}
	if strings.Join(systemd.calls, "|") != strings.Join(want, "|") {
		t.Fatalf("calls %v", systemd.calls)
	}
	for _, say := range []string{"journalctl --user -u silo-runner.service", "sudo loginctl enable-linger ci"} {
		if !strings.Contains(out.String(), say) {
			t.Fatalf("output lacks %q:\n%s", say, out)
		}
	}
	info, _ := os.Stat(setup.unitPath())
	if info.Mode().Perm()&0o022 != 0 {
		t.Fatalf("mode %v", info.Mode())
	}
}

func TestRegisterSaysLingeringIsOnWhenItIs(t *testing.T) {
	setup, systemd, out, program, config := serviceFixture(t)
	systemd.linger = "yes"
	if err := RegisterService(setup, program, config, silo, defaultEngineUnit); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out.String(), "linger: yes") || strings.Contains(out.String(), "sudo") {
		t.Fatal(out.String())
	}
}

func TestRegisterRefusesRootAndMissingFilesAndSomeoneElsesUnit(t *testing.T) {
	setup, systemd, _, program, config := serviceFixture(t)
	root := setup
	root.Euid = 0
	if err := RegisterService(root, program, config, silo, defaultEngineUnit); err == nil || !strings.Contains(err.Error(), "root") {
		t.Fatalf("err = %v", err)
	}
	if err := RegisterService(setup, program, filepath.Join(filepath.Dir(config), "missing.yaml"), silo, defaultEngineUnit); err == nil {
		t.Fatal("a missing configuration was accepted")
	}
	if err := os.MkdirAll(setup.UnitDir, 0o755); err != nil {
		t.Fatal(err)
	}
	foreign := "[Service]\nExecStart=/usr/bin/true\n"
	if err := os.WriteFile(setup.unitPath(), []byte(foreign), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := RegisterService(setup, program, config, silo, defaultEngineUnit); err == nil || !strings.Contains(err.Error(), "left alone") {
		t.Fatalf("err = %v", err)
	}
	if err := UnregisterService(setup); err == nil {
		t.Fatal("removed a unit the helper did not write")
	}
	if got, _ := os.ReadFile(setup.unitPath()); string(got) != foreign || len(systemd.calls) != 0 {
		t.Fatalf("unit %q calls %v", got, systemd.calls)
	}
}

func TestRegisteringAgainRewritesItsOwnUnit(t *testing.T) {
	setup, _, _, program, config := serviceFixture(t)
	if err := RegisterService(setup, program, config, silo, defaultEngineUnit); err != nil {
		t.Fatal(err)
	}
	if err := RegisterService(setup, program, config, silo, "org.silo.dev.engine.service"); err != nil {
		t.Fatal(err)
	}
	if got, _ := os.ReadFile(setup.unitPath()); !strings.Contains(string(got), "After=org.silo.dev.engine.service") {
		t.Fatal(string(got))
	}
}

func TestASystemctlFailureNamesTheUserManagerFix(t *testing.T) {
	setup, systemd, _, program, config := serviceFixture(t)
	systemd.fail = map[string]error{"systemctl --user enable": errors.New("exit 1")}
	err := RegisterService(setup, program, config, silo, defaultEngineUnit)
	if err == nil || !strings.Contains(err.Error(), "XDG_RUNTIME_DIR") || !strings.Contains(err.Error(), "boom") {
		t.Fatalf("err = %v", err)
	}
}

func TestUnregisterStopsDisablesAndRemovesTheUnit(t *testing.T) {
	setup, systemd, out, program, config := serviceFixture(t)
	if err := UnregisterService(setup); err != nil || !strings.Contains(out.String(), "is not registered") {
		t.Fatalf("err %v out %s", err, out)
	}
	if err := RegisterService(setup, program, config, silo, defaultEngineUnit); err != nil {
		t.Fatal(err)
	}
	systemd.calls = nil
	if err := UnregisterService(setup); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(setup.unitPath()); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the unit is still there: %v", err)
	}
	if strings.Join(systemd.calls, "|") != "systemctl --user disable --now silo-runner.service|systemctl --user daemon-reload" {
		t.Fatalf("calls %v", systemd.calls)
	}
}

func TestTheUnitDirectoryFollowsXDGConfigHome(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "/xdg/config")
	setup, err := DefaultServiceSetup(os.Stdout)
	if err != nil || setup.UnitDir != "/xdg/config/systemd/user" || setup.Euid != os.Geteuid() {
		t.Fatalf("setup %+v err %v", setup, err)
	}
	t.Setenv("XDG_CONFIG_HOME", "relative")
	home, _ := os.UserHomeDir()
	if setup, _ = DefaultServiceSetup(os.Stdout); setup.UnitDir != filepath.Join(home, ".config", "systemd", "user") {
		t.Fatalf("unit dir %s", setup.UnitDir)
	}
}

func TestTheUnitGivesTheHelperTheAbsoluteSiloPathBecauseItsPathLacksLocalBin(t *testing.T) {
	unit, err := ServiceUnit("/a/silo-runner", "/a/c.yaml", "/home/ci/.local/bin/silo", defaultEngineUnit)
	if err != nil || !strings.Contains(unit, `-config "/a/c.yaml" -silo "/home/ci/.local/bin/silo"`+"\n") {
		t.Fatalf("err %v\n%s", err, unit)
	}
	for _, bad := range []string{"silo", "./silo", "", "~/.local/bin/silo"} {
		if _, err := ServiceUnit("/a/silo-runner", "/a/c.yaml", bad, defaultEngineUnit); err == nil || !strings.Contains(err.Error(), "silo.path") {
			t.Fatalf("accepted silo %q: %v", bad, err)
		}
	}
}

func TestRegisterResolvesABareSiloFromThePathOfTheRegisteringShell(t *testing.T) {
	bin := t.TempDir()
	path := filepath.Join(bin, "silo")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin)
	got, err := ResolveSilo("silo", exec.LookPath)
	if err != nil || got != path {
		t.Fatalf("got %q err %v", got, err)
	}
	// An absolute path needs no lookup.
	t.Setenv("PATH", "")
	if got, err = ResolveSilo(path, exec.LookPath); err != nil || got != path {
		t.Fatalf("got %q err %v", got, err)
	}
}

func TestASiloThatCannotBeResolvedIsRefusedWithTheFix(t *testing.T) {
	dir := t.TempDir()
	plain := filepath.Join(dir, "plain")
	_ = os.WriteFile(plain, []byte("x"), 0o644)
	t.Setenv("PATH", dir)
	for name, path := range map[string]string{"not on PATH": "silo", "empty": "", "relative": "bin/silo", "missing": filepath.Join(dir, "gone"), "not executable": plain, "a directory": dir} {
		_, err := ResolveSilo(path, exec.LookPath)
		if err == nil || !strings.Contains(err.Error(), "silo.path") {
			t.Fatalf("%s: err = %v", name, err)
		}
	}
	setup, systemd, _, program, config := serviceFixture(t)
	if err := RegisterService(setup, program, config, "silo", defaultEngineUnit); err == nil {
		t.Fatal("registered a unit with a bare silo")
	}
	if _, err := os.Stat(setup.unitPath()); err == nil || len(systemd.calls) != 0 {
		t.Fatalf("a unit was written or systemctl called: %v", systemd.calls)
	}
}
