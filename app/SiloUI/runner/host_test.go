package main

import (
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const meminfo = "MemTotal:       32768000 kB\nMemFree:         1000 kB\nMemAvailable:   20000000 kB\n"

func linuxHost(memKiB int64, cpus int, kvm error) Host {
	return Host{GOOS: "linux", CPUs: cpus, MemTotalKiB: func() (int64, error) { return memKiB, nil }, OpenKVM: func() error { return kvm }}
}

func hostConfig(t *testing.T, extra string) *Config {
	t.Helper()
	text := "github: {configUrl: https://github.com/o/r, auth: {tokenEnv: T}}\nsilo: {projectDir: " + t.TempDir() + "}\n" + extra +
		"scaleSets:\n  - {label: silo-linux, recipe: silo-linux, os: linux}\n"
	cfg, err := ParseConfig([]byte(text), "test.yaml")
	if err != nil {
		t.Fatal(err)
	}
	return cfg
}

func TestMemTotalIsReadFromProcMeminfo(t *testing.T) {
	kib, err := ParseMemTotalKiB(strings.NewReader(meminfo))
	if err != nil || kib != 32768000 {
		t.Fatalf("kib %d err %v", kib, err)
	}
	for _, bad := range []string{"", "MemFree: 1 kB\n", "MemTotal: lots kB\n", "MemTotal: 5 MB\n", "MemTotal: 0 kB\n"} {
		if _, err := ParseMemTotalKiB(strings.NewReader(bad)); err == nil {
			t.Fatalf("accepted %q", bad)
		}
	}
	if _, err := ParseMemTotalKiB(io.NopCloser(strings.NewReader(""))); err == nil {
		t.Fatal("empty input")
	}
}

func TestTheDefaultBudgetIsThreeQuartersOfMemoryAndOneComputerPerCPU(t *testing.T) {
	got := DefaultLinuxBudget(32*1024*1024, 8)
	if got.MemoryGiB != 24 || got.Count != 8 {
		t.Fatalf("budget %+v", got)
	}
	// Rounds down, and always allows one computer.
	if got := DefaultLinuxBudget(10*1024*1024+512, 0); got.MemoryGiB != 7 || got.Count != 1 {
		t.Fatalf("budget %+v", got)
	}
}

func TestALinuxHostGetsItsBudgetFromTheMachineUnlessTheFileSetsIt(t *testing.T) {
	cfg := hostConfig(t, "")
	report, err := ApplyHost(cfg, linuxHost(32*1024*1024, 8, nil))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.LinuxHost != (LinuxBudget{MemoryGiB: 24, Count: 8}) || !report.LinuxHost || !report.MemoryFromHost || !report.CountFromHost {
		t.Fatalf("cfg %+v report %+v", cfg.LinuxHost, report)
	}
	if line := report.Describe(); !strings.Contains(line, "at most 8 at once (from this host)") || !strings.Contains(line, "24 GiB (from this host)") {
		t.Fatal(line)
	}
	// A field the file sets wins; the other still comes from the host.
	cfg = hostConfig(t, "linuxBudget: {memoryGiB: 12}\n")
	report, err = ApplyHost(cfg, linuxHost(32*1024*1024, 4, nil))
	if err != nil || cfg.LinuxHost != (LinuxBudget{MemoryGiB: 12, Count: 4}) || report.MemoryFromHost || !report.CountFromHost {
		t.Fatalf("cfg %+v report %+v err %v", cfg.LinuxHost, report, err)
	}
	// With both set the host is not even read.
	cfg = hostConfig(t, "linuxBudget: {memoryGiB: 16, count: 2}\n")
	host := linuxHost(0, 1, nil)
	host.MemTotalKiB = func() (int64, error) { return 0, errors.New("must not be read") }
	if _, err := ApplyHost(cfg, host); err != nil || cfg.LinuxHost != (LinuxBudget{MemoryGiB: 16, Count: 2}) {
		t.Fatalf("cfg %+v err %v", cfg.LinuxHost, err)
	}
}

func TestAHostWhoseMemoryCannotBeReadAsksForTheBudgetInTheFile(t *testing.T) {
	host := linuxHost(0, 4, nil)
	host.MemTotalKiB = func() (int64, error) { return 0, errors.New("no /proc") }
	if _, err := ApplyHost(hostConfig(t, ""), host); err == nil || !strings.Contains(err.Error(), "set linuxBudget") {
		t.Fatalf("err = %v", err)
	}
}

func TestABudgetTooSmallForOneComputerIsRefused(t *testing.T) {
	// The shipped recipe is 8 GiB; a 4 GiB host gets a 3 GiB budget.
	cfg := hostConfig(t, "")
	dir := filepath.Join(cfg.Silo.ProjectDir, ".silo", "recipes")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "silo-linux.yaml"), []byte("os: linux\nmemory: 8\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := ApplyHost(cfg, linuxHost(4*1024*1024, 4, nil))
	if err == nil || !strings.Contains(err.Error(), "3 GiB") || !strings.Contains(err.Error(), "8 GiB") {
		t.Fatalf("err = %v", err)
	}
}

func TestKVMThatIsMissingOrNotUsableIsNamedClearly(t *testing.T) {
	_, err := ApplyHost(hostConfig(t, ""), linuxHost(32*1024*1024, 4, &os.PathError{Op: "open", Path: "/dev/kvm", Err: os.ErrNotExist}))
	if err == nil || !strings.Contains(err.Error(), "/dev/kvm does not exist") {
		t.Fatalf("err = %v", err)
	}
	_, err = ApplyHost(hostConfig(t, ""), linuxHost(32*1024*1024, 4, &os.PathError{Op: "open", Path: "/dev/kvm", Err: os.ErrPermission}))
	if err == nil || !strings.Contains(err.Error(), "kvm group") {
		t.Fatalf("err = %v", err)
	}
	other := errors.New("device busy")
	if _, err = ApplyHost(hostConfig(t, ""), linuxHost(32*1024*1024, 4, other)); !errors.Is(err, other) {
		t.Fatalf("err = %v", err)
	}
}

func TestALinuxHostRefusesAMacOSScaleSet(t *testing.T) {
	cfg := hostConfig(t, "")
	cfg.ScaleSets = append(cfg.ScaleSets, ScaleSetSpec{Label: "silo-macos", Recipe: "silo-macos", OS: "macos", MaxRunners: 1})
	if _, err := ApplyHost(cfg, linuxHost(32*1024*1024, 4, nil)); err == nil || !strings.Contains(err.Error(), "cannot run macOS") {
		t.Fatalf("err = %v", err)
	}
}

func TestNothingChangesOnAMacOrWithAnotherMachinesEngine(t *testing.T) {
	cfg := hostConfig(t, "")
	mac := Host{GOOS: "darwin", OpenKVM: func() error { t.Fatal("KVM is not a Mac's"); return nil }}
	if report, err := ApplyHost(cfg, mac); err != nil || report.LinuxHost || cfg.LinuxHost != (LinuxBudget{}) {
		t.Fatalf("report %+v err %v budget %+v", report, err, cfg.LinuxHost)
	}
	// A silo.device points the CLI at an engine on another machine: this host's KVM and memory say
	// nothing about it.
	cfg = hostConfig(t, "")
	cfg.Silo.Device = "other"
	host := linuxHost(32*1024*1024, 4, errors.New("must not be asked"))
	if report, err := ApplyHost(cfg, host); err != nil || report.LinuxHost || cfg.LinuxHost != (LinuxBudget{}) {
		t.Fatalf("report %+v err %v budget %+v", report, err, cfg.LinuxHost)
	}
	if (Host{}).IsLinuxHost(cfg) {
		t.Fatal("the zero Host is not a Linux host")
	}
}

func TestTheRealHostReadsThisMachine(t *testing.T) {
	host := RealHost()
	if host.GOOS == "" || host.CPUs < 1 || host.MemTotalKiB == nil || host.OpenKVM == nil {
		t.Fatalf("host %+v", host)
	}
}

func TestAnEmptyTokenVariableIsNamedBeforeAnyGitHubCall(t *testing.T) {
	t.Setenv("R2_TEST_TOKEN", "")
	cfg, err := ParseConfig([]byte("github: {configUrl: https://github.com/o/r, auth: {tokenEnv: R2_TEST_TOKEN}}\nscaleSets: [{label: a, recipe: a}]\n"), "t.yaml")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := NewDeps(cfg, quiet()); err == nil || !strings.Contains(err.Error(), "R2_TEST_TOKEN, which is empty") || !strings.Contains(err.Error(), "silo-runner/env") {
		t.Fatalf("err = %v", err)
	}
}

func TestTheLinuxExampleConfigurationIsValidAndTakesItsBudgetFromTheHost(t *testing.T) {
	data, err := os.ReadFile("examples/config-linux.yaml")
	if err != nil {
		t.Fatal(err)
	}
	cfg, err := ParseConfig(data, "examples/config-linux.yaml")
	if err != nil {
		t.Fatal(err)
	}
	if cfg.LinuxHost != (LinuxBudget{}) || len(cfg.ScaleSets) != 1 || cfg.ScaleSets[0].OS != "linux" {
		t.Fatalf("config %+v", cfg)
	}
	report, err := ApplyHost(cfg, linuxHost(32*1024*1024, 8, nil))
	if err != nil || !report.MemoryFromHost || !report.CountFromHost {
		t.Fatalf("report %+v err %v", report, err)
	}
}
