package main

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
)

// Host is what the helper asks of the machine it runs on. Tests replace it; the zero value is
// "not a Linux host", which checks nothing.
type Host struct {
	// GOOS is the operating system the helper runs on.
	GOOS string
	// GOARCH is the architecture the helper runs on, as runtime.GOARCH names it.
	GOARCH string
	// CPUs is the number of CPUs the helper may use.
	CPUs int
	// MemTotalKiB reads MemTotal from /proc/meminfo.
	MemTotalKiB func() (int64, error)
	// OpenKVM opens /dev/kvm for reading and writing, the way the engine's virtual machines do.
	OpenKVM func() error
}

// RealHost is the machine the helper is running on.
func RealHost() Host {
	return Host{
		GOOS:   runtime.GOOS,
		GOARCH: runtime.GOARCH,
		CPUs:   runtime.NumCPU(),
		MemTotalKiB: func() (int64, error) {
			file, err := os.Open("/proc/meminfo")
			if err != nil {
				return 0, err
			}
			defer file.Close()
			return ParseMemTotalKiB(file)
		},
		OpenKVM: func() error {
			file, err := os.OpenFile("/dev/kvm", os.O_RDWR, 0)
			if err != nil {
				return err
			}
			return file.Close()
		},
	}
}

// ParseMemTotalKiB reads the MemTotal line of /proc/meminfo.
func ParseMemTotalKiB(r io.Reader) (int64, error) {
	scanner := bufio.NewScanner(r)
	for scanner.Scan() {
		rest, ok := strings.CutPrefix(scanner.Text(), "MemTotal:")
		if !ok {
			continue
		}
		fields := strings.Fields(rest)
		if len(fields) != 2 || fields[1] != "kB" {
			return 0, fmt.Errorf("cannot read MemTotal from %q", scanner.Text())
		}
		kib, err := strconv.ParseInt(fields[0], 10, 64)
		if err != nil || kib <= 0 {
			return 0, fmt.Errorf("cannot read MemTotal from %q", scanner.Text())
		}
		return kib, nil
	}
	if err := scanner.Err(); err != nil {
		return 0, err
	}
	return 0, errors.New("/proc/meminfo has no MemTotal line")
}

// DefaultLinuxBudget is the budget a Linux host gets when linuxBudget does not set one: three
// quarters of the host's memory (the rest is the host's own, the page cache and each virtual
// machine's overhead) and at most one computer per CPU. It comes from the machine's size, not
// from what is free at the moment, so a restart of the helper with computers already running
// computes the same budget as the first start.
func DefaultLinuxBudget(memTotalKiB int64, cpus int) LinuxBudget {
	const kibPerGiB = 1 << 20
	return LinuxBudget{MemoryGiB: int(memTotalKiB * 3 / 4 / kibPerGiB), Count: max(cpus, 1)}
}

// IsLinuxHost reports whether the helper runs on the Linux machine whose engine it drives. With a
// `silo.device` the engine is on another machine and nothing here describes it.
func (h Host) IsLinuxHost(cfg *Config) bool {
	return h.GOOS == "linux" && cfg.Silo.Device == ""
}

// HostReport says what ApplyHost decided, for the log.
type HostReport struct {
	// LinuxHost is true when the host checks ran.
	LinuxHost bool
	// Budget is the Linux budget in force.
	Budget LinuxBudget
	// MemoryFromHost and CountFromHost say which fields came from the machine and not the file.
	MemoryFromHost, CountFromHost bool
}

// ApplyHost checks a Linux host and fills in its Linux budget. On a Linux host it refuses a
// macOS scale set, requires a usable /dev/kvm, and gives an unset linuxBudget field the host's
// default (DefaultLinuxBudget). The budget must fit at least one computer of every Linux
// recipe, or no job would ever start. Anywhere else it changes nothing.
func ApplyHost(cfg *Config, host Host) (HostReport, error) {
	if !host.IsLinuxHost(cfg) {
		return HostReport{Budget: cfg.LinuxHost}, nil
	}
	report := HostReport{LinuxHost: true}
	largest := 0
	for i, set := range cfg.ScaleSets {
		if set.OS == "macos" {
			return report, fmt.Errorf("scaleSets[%d]: a Linux host cannot run macOS computers; remove %q or run silo-runner on a Mac", i, set.Label)
		}
		if err := checkRecipeArchitecture(cfg.Silo.ProjectDir, set, host.GOARCH); err != nil {
			return report, err
		}
		largest = max(largest, RecipeMemoryGiB(cfg.Silo.ProjectDir, set.Recipe, set.OS))
	}
	if err := host.OpenKVM(); err != nil {
		return report, kvmError(err)
	}
	budget := cfg.LinuxHost
	if budget.MemoryGiB == 0 || budget.Count == 0 {
		total, err := host.MemTotalKiB()
		if err != nil {
			return report, fmt.Errorf("cannot read this host's memory to size linuxBudget (set linuxBudget.memoryGiB and linuxBudget.count yourself): %w", err)
		}
		defaults := DefaultLinuxBudget(total, host.CPUs)
		if budget.MemoryGiB == 0 {
			budget.MemoryGiB, report.MemoryFromHost = defaults.MemoryGiB, true
		}
		if budget.Count == 0 {
			budget.Count, report.CountFromHost = defaults.Count, true
		}
	}
	if budget.MemoryGiB < largest {
		return report, fmt.Errorf("linuxBudget.memoryGiB is %d GiB%s but a computer of your recipes needs %d GiB, so no job could start; give the recipe less memory or set linuxBudget.memoryGiB to at least %d",
			budget.MemoryGiB, fromHost(report), largest, largest)
	}
	cfg.LinuxHost = budget
	report.Budget = budget
	return report, nil
}

func fromHost(report HostReport) string {
	switch {
	case report.MemoryFromHost:
		return " (three quarters of this host's memory)"
	}
	return ""
}

// runnerArch is the name GitHub's runner archives and the recipe pins give a Go architecture.
func runnerArch(goarch string) string {
	switch goarch {
	case "amd64":
		return "x64"
	case "arm64":
		return "arm64"
	}
	return ""
}

// checkRecipeArchitecture refuses an installed Linux recipe that pins the GitHub runner for one
// architecture other than the host's. Recipes installed before the dual pin carry a single arm64
// digest (install-recipes keeps an existing file), and a job on an x86_64 host would fail after a
// long recipe build with no named cause. A recipe that is missing, does not pin a runner, or pins
// both architectures is not this check's business.
func checkRecipeArchitecture(projectDir string, set ScaleSetSpec, goarch string) error {
	want := runnerArch(goarch)
	if want == "" || set.OS == "macos" {
		return nil
	}
	raw, err := os.ReadFile(filepath.Join(projectDir, ".silo", "recipes", set.Recipe+".yaml"))
	if err != nil {
		return nil
	}
	pin, err := ParsePin(string(raw))
	if err != nil || pin.OS != "linux" || len(pin.SHA256) != 1 {
		return nil
	}
	for have := range pin.SHA256 {
		if have != want {
			return fmt.Errorf("the recipe %q downloads the GitHub runner for %s but this host is %s, so a computer built from it could not run a job: reinstall the shipped recipe with `silo-runner install-recipes -replace -config <file>` (it keeps your copy as %s.yaml.bak; copy your own tools back)",
				set.Recipe, have, want, set.Recipe)
		}
	}
	return nil
}

// kvmError explains an unusable /dev/kvm.
func kvmError(err error) error {
	switch {
	case errors.Is(err, os.ErrNotExist):
		return errors.New("/dev/kvm does not exist: this machine cannot run Silo's Linux computers. Turn on hardware virtualization (and on a virtual machine, nested virtualization) and make sure the kvm kernel module is loaded")
	case errors.Is(err, os.ErrPermission):
		return errors.New("/dev/kvm is not readable and writable by this account: add the account that runs silod and silo-runner to the kvm group (sudo usermod -aG kvm \"$(id -un)\"), then log in again and restart the user manager's services")
	}
	return fmt.Errorf("cannot open /dev/kvm: %w", err)
}

// Describe is the line the helper logs about the budget.
func (r HostReport) Describe() string {
	where := func(derived bool) string {
		if derived {
			return "from this host"
		}
		return "from the configuration"
	}
	return fmt.Sprintf("Linux computers: at most %d at once (%s) using at most %d GiB (%s)",
		r.Budget.Count, where(r.CountFromHost), r.Budget.MemoryGiB, where(r.MemoryFromHost))
}
