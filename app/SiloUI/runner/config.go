package main

import (
	"bytes"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

// Config is the helper's configuration file. YAML and JSON are both accepted (JSON is YAML),
// and unknown keys are errors.
type Config struct {
	GitHub     GitHubConfig   `yaml:"github"`
	Silo       SiloConfig     `yaml:"silo"`
	StateDir   string         `yaml:"stateDir"`
	Limits     Limits         `yaml:"limits"`
	Runner     RunnerConfig   `yaml:"runner"`
	LinuxHost  LinuxBudget    `yaml:"linuxBudget"`
	ScaleSets  []ScaleSetSpec `yaml:"scaleSets"`
	Refresh    RefreshConfig  `yaml:"runnerRefresh"`
	configPath string
}

// GitHubConfig says where the scale sets live and how the helper signs in.
type GitHubConfig struct {
	// ConfigURL is the repository (https://github.com/OWNER/REPO) or organization
	// (https://github.com/ORG) the scale sets belong to.
	ConfigURL   string     `yaml:"configUrl"`
	Auth        AuthConfig `yaml:"auth"`
	RunnerGroup string     `yaml:"runnerGroup"`
}

// AuthConfig is a GitHub App or a personal access token read from an environment variable.
type AuthConfig struct {
	App      *AppAuth `yaml:"app"`
	TokenEnv string   `yaml:"tokenEnv"`
}

// AppAuth is a GitHub App installation.
type AppAuth struct {
	ClientID       string `yaml:"clientId"`
	InstallationID int64  `yaml:"installationId"`
	PrivateKeyFile string `yaml:"privateKeyFile"`
}

// SiloConfig says how to reach Silo through its command line.
type SiloConfig struct {
	// Path is the silo command (silo, or silo-dev for a development build).
	Path string `yaml:"path"`
	// ProjectDir holds .silo/recipes; silo runs there.
	ProjectDir string `yaml:"projectDir"`
	Socket     string `yaml:"socket"`
	Device     string `yaml:"device"`
}

// Limits are the host-side guards.
type Limits struct {
	MinFreeDiskGiB int      `yaml:"minFreeDiskGiB"`
	DiskPath       string   `yaml:"diskPath"`
	ReadyTimeout   Duration `yaml:"readyTimeout"`
	StartAttempts  int      `yaml:"startAttempts"`
	IdleTimeout    Duration `yaml:"idleTimeout"`
	ListenerStall  Duration `yaml:"listenerStall"`
}

// RunnerConfig overrides where GitHub's runner is started inside a computer, per operating system.
type RunnerConfig struct {
	LinuxStart []string `yaml:"linuxStart"`
	MacosStart []string `yaml:"macosStart"`
}

// LinuxBudget bounds the Linux computers the helper starts; zero means no bound beyond maxRunners.
type LinuxBudget struct {
	MemoryGiB int `yaml:"memoryGiB"`
	Count     int `yaml:"count"`
}

// RefreshConfig controls the runner version check.
type RefreshConfig struct {
	Disabled   bool     `yaml:"disabled"`
	CheckEvery Duration `yaml:"checkEvery"`
	// AfterDays is how old a new runner release must be before the recipe moves to it.
	AfterDays int `yaml:"afterDays"`
}

// ScaleSetSpec is one scale set: one label, one recipe.
type ScaleSetSpec struct {
	Label      string     `yaml:"label"`
	Recipe     string     `yaml:"recipe"`
	OS         string     `yaml:"os"`
	MaxRunners int        `yaml:"maxRunners"`
	KeepFailed KeepFailed `yaml:"keepFailed"`
}

// KeepFailed bounds the stopped computers kept after failed jobs. Count left out means the
// default for the operating system (see defaultKeepCount); an explicit 0 keeps none, so a failed
// job's computer is deleted like any other.
type KeepFailed struct {
	Count *int `yaml:"count"`
	Days  int  `yaml:"days"`
}

// Limit is how many failed computers are kept.
func (k KeepFailed) Limit() int {
	if k.Count == nil {
		return 0
	}
	return *k.Count
}

// defaultKeepCount is how many failed computers a scale set keeps when keepFailed.count is not
// set. A stopped computer holds no virtual machine slot (Apple's two-at-once limit counts running
// macOS computers only) but it holds its disk, and a macOS computer's disk is much larger.
func defaultKeepCount(os string) int {
	if os == "macos" {
		return 1
	}
	return 3
}

// Duration is a duration written as text such as 90s or 10m.
type Duration time.Duration

// UnmarshalYAML reads a duration string.
func (d *Duration) UnmarshalYAML(node *yaml.Node) error {
	var text string
	if err := node.Decode(&text); err != nil {
		return err
	}
	parsed, err := time.ParseDuration(text)
	if err != nil {
		return fmt.Errorf("line %d: %q is not a duration like 90s or 10m", node.Line, text)
	}
	*d = Duration(parsed)
	return nil
}

// Std returns the standard duration.
func (d Duration) Std() time.Duration { return time.Duration(d) }

// The longest a runner may go without an update under GitHub's rule for self-hosted runners
// with updates disabled.
const runnerUpdateDeadlineDays = 30

// LoadConfig reads, defaults and validates a configuration file.
func LoadConfig(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("cannot read the configuration: %w", err)
	}
	return ParseConfig(data, path)
}

// ParseConfig parses configuration text. path locates relative file names.
func ParseConfig(data []byte, path string) (*Config, error) {
	decoder := yaml.NewDecoder(bytes.NewReader(data))
	decoder.KnownFields(true)
	var cfg Config
	if err := decoder.Decode(&cfg); err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	cfg.configPath = path
	cfg.applyDefaults()
	if err := cfg.Validate(); err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	return &cfg, nil
}

// expandHome replaces a leading ~/ with the home directory.
func expandHome(path, home string) string {
	if rest, ok := strings.CutPrefix(path, "~/"); ok && home != "" {
		return filepath.Join(home, rest)
	}
	return path
}

func (c *Config) applyDefaults() {
	home, _ := os.UserHomeDir()
	c.Silo.ProjectDir = expandHome(c.Silo.ProjectDir, home)
	c.Silo.Path = expandHome(c.Silo.Path, home)
	c.StateDir = expandHome(c.StateDir, home)
	c.Limits.DiskPath = expandHome(c.Limits.DiskPath, home)
	if c.GitHub.Auth.App != nil {
		c.GitHub.Auth.App.PrivateKeyFile = expandHome(c.GitHub.Auth.App.PrivateKeyFile, home)
	}
	if c.Silo.Path == "" {
		c.Silo.Path = "silo"
	}
	if c.Silo.ProjectDir == "" {
		c.Silo.ProjectDir = filepath.Join(home, ".silo-runner", "project")
	}
	if c.StateDir == "" {
		c.StateDir = filepath.Join(home, ".silo-runner", "state")
	}
	if c.GitHub.RunnerGroup == "" {
		c.GitHub.RunnerGroup = "default"
	}
	if c.Limits.MinFreeDiskGiB == 0 {
		c.Limits.MinFreeDiskGiB = 20
	}
	if c.Limits.DiskPath == "" {
		c.Limits.DiskPath = home
	}
	if c.Limits.ReadyTimeout == 0 {
		c.Limits.ReadyTimeout = Duration(3 * time.Minute)
	}
	if c.Limits.StartAttempts == 0 {
		c.Limits.StartAttempts = 3
	}
	if c.Limits.IdleTimeout == 0 {
		c.Limits.IdleTimeout = Duration(10 * time.Minute)
	}
	if c.Limits.ListenerStall == 0 {
		c.Limits.ListenerStall = Duration(5 * time.Minute)
	}
	if len(c.Runner.LinuxStart) == 0 {
		c.Runner.LinuxStart = []string{"/opt/actions-runner/silo-run-jit"}
	}
	if len(c.Runner.MacosStart) == 0 {
		c.Runner.MacosStart = []string{"/Users/Shared/actions-runner/silo-run-jit"}
	}
	if c.Refresh.CheckEvery == 0 {
		c.Refresh.CheckEvery = Duration(24 * time.Hour)
	}
	if c.Refresh.AfterDays == 0 {
		c.Refresh.AfterDays = 3
	}
	for i := range c.ScaleSets {
		set := &c.ScaleSets[i]
		if set.OS == "" {
			set.OS = "linux"
		}
		if set.MaxRunners == 0 {
			set.MaxRunners = 1
		}
		if set.KeepFailed.Count == nil {
			count := defaultKeepCount(set.OS)
			set.KeepFailed.Count = &count
		}
		if set.KeepFailed.Days == 0 {
			set.KeepFailed.Days = 7
		}
	}
}

// Validate checks the configuration.
func (c *Config) Validate() error {
	if _, err := ParseTarget(c.GitHub.ConfigURL); err != nil {
		return err
	}
	if err := c.GitHub.Auth.validate(); err != nil {
		return err
	}
	if len(c.ScaleSets) == 0 {
		return errors.New("scaleSets needs at least one entry: {label, recipe, os, maxRunners}")
	}
	labels := map[string]bool{}
	for i, set := range c.ScaleSets {
		where := fmt.Sprintf("scaleSets[%d]", i)
		if set.Label == "" || set.Recipe == "" {
			return fmt.Errorf("%s needs a label and a recipe", where)
		}
		if strings.ContainsAny(set.Label, " ,/\\") || strings.HasPrefix(set.Label, "-") {
			return fmt.Errorf("%s: the label %q must be one word without spaces, commas or slashes", where, set.Label)
		}
		if labels[set.Label] {
			return fmt.Errorf("%s: the label %q is used twice; one label means one recipe", where, set.Label)
		}
		labels[set.Label] = true
		if set.OS != "linux" && set.OS != "macos" {
			return fmt.Errorf("%s: os is linux or macos, not %q", where, set.OS)
		}
		if set.MaxRunners < 1 {
			return fmt.Errorf("%s: maxRunners must be at least 1", where)
		}
		if set.OS == "macos" && set.MaxRunners > MacOSVMLimit {
			return fmt.Errorf("%s: Apple allows %d macOS virtual machines at once, so maxRunners cannot exceed %d", where, MacOSVMLimit, MacOSVMLimit)
		}
		if set.KeepFailed.Limit() < 0 || set.KeepFailed.Days < 0 {
			return fmt.Errorf("%s: keepFailed count and days cannot be negative", where)
		}
	}
	if c.Refresh.AfterDays < 0 || c.Refresh.AfterDays >= runnerUpdateDeadlineDays-2 {
		return fmt.Errorf("runnerRefresh.afterDays must be between 0 and %d: GitHub requires an update within %d days of a release", runnerUpdateDeadlineDays-3, runnerUpdateDeadlineDays)
	}
	if c.Limits.StartAttempts < 1 {
		return errors.New("limits.startAttempts must be at least 1")
	}
	return nil
}

func (a AuthConfig) validate() error {
	hasApp, hasToken := a.App != nil, a.TokenEnv != ""
	switch {
	case hasApp && hasToken:
		return errors.New("github.auth takes a GitHub App or a tokenEnv, not both")
	case !hasApp && !hasToken:
		return errors.New("github.auth needs a GitHub App (app.clientId, app.installationId, app.privateKeyFile) or tokenEnv, the name of an environment variable holding a personal access token")
	case hasApp:
		if a.App.ClientID == "" || a.App.InstallationID == 0 || a.App.PrivateKeyFile == "" {
			return errors.New("github.auth.app needs clientId, installationId and privateKeyFile")
		}
	}
	return nil
}

// Target is what the configuration URL names.
type Target struct {
	Host  string
	Owner string
	// Repo is empty for an organization.
	Repo string
}

// IsOrg reports whether the target is an organization.
func (t Target) IsOrg() bool { return t.Repo == "" }

// ParseTarget reads a repository or organization URL. Enterprises are not supported.
func ParseTarget(raw string) (Target, error) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.Scheme != "https" || u.Host == "" {
		return Target{}, fmt.Errorf("github.configUrl %q must be an https URL of a repository (https://github.com/OWNER/REPO) or an organization (https://github.com/ORG)", raw)
	}
	parts := strings.Split(strings.Trim(u.Path, "/"), "/")
	switch {
	case len(parts) == 1 && parts[0] != "":
		return Target{Host: u.Host, Owner: parts[0]}, nil
	case len(parts) == 2 && !strings.EqualFold(parts[0], "enterprises") && parts[1] != "":
		return Target{Host: u.Host, Owner: parts[0], Repo: strings.TrimSuffix(parts[1], ".git")}, nil
	}
	return Target{}, fmt.Errorf("github.configUrl %q must name one repository or one organization; enterprises are not supported", raw)
}
