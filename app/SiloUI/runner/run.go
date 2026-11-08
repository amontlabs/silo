package main

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"path/filepath"
	"sync"
	"time"

	"github.com/actions/scaleset"
	"github.com/actions/scaleset/listener"
)

// ScaleAPI is the scaleset client as the supervisor uses it.
type ScaleAPI interface {
	Actions
	GetRunnerGroupByName(ctx context.Context, name string) (*scaleset.RunnerGroup, error)
	GetRunnerScaleSet(ctx context.Context, runnerGroupID int, name string) (*scaleset.RunnerScaleSet, error)
	CreateRunnerScaleSet(ctx context.Context, set *scaleset.RunnerScaleSet) (*scaleset.RunnerScaleSet, error)
	UpdateRunnerScaleSet(ctx context.Context, id int, set *scaleset.RunnerScaleSet) (*scaleset.RunnerScaleSet, error)
	OpenSession(ctx context.Context, scaleSetID int, owner string) (Session, error)
}

// Deps are the things Run talks to; tests replace them.
type Deps struct {
	Silo     Silo
	API      ScaleAPI
	Checker  AccessChecker
	Releases ReleaseSource
	Disk     func(path string) (int, error)
	// Host describes the machine; the zero value is not a Linux host and checks nothing.
	Host   Host
	Logger *slog.Logger
	Now    func() time.Time
}

// tickEvery is how often the fleet plans on its own, retrying whatever failed.
const tickEvery = 30 * time.Second

// defaultRunnerGroupID is the id of every repository's and organization's default runner group.
const defaultRunnerGroupID = 1

// NewDeps builds the real dependencies from a configuration.
func NewDeps(cfg *Config, logger *slog.Logger) (*Deps, error) {
	target, err := ParseTarget(cfg.GitHub.ConfigURL)
	if err != nil {
		return nil, err
	}
	if env := cfg.GitHub.Auth.TokenEnv; env != "" && os.Getenv(env) == "" {
		return nil, fmt.Errorf("github.auth.tokenEnv names %s, which is empty in this environment (under systemd, put %s=... in the file ~/.config/silo-runner/env, mode 0600)", env, env)
	}
	api, err := newScaleClient(cfg)
	if err != nil {
		return nil, err
	}
	token, err := TokenSource(cfg.GitHub.Auth, APIBaseFor(target.Host), nil, time.Now)
	if err != nil {
		return nil, err
	}
	return &Deps{
		Silo: &CLI{Path: cfg.Silo.Path, Dir: cfg.Silo.ProjectDir, Socket: cfg.Silo.Socket, Device: cfg.Silo.Device, Timeout: 10 * time.Minute},
		API:  scaleAPI{api},
		Checker: &RESTChecker{
			APIBase: APIBaseFor(target.Host),
			Token:   token,
		},
		Releases: GitHubReleases{},
		Host:     RealHost(),
		Logger:   logger,
		Now:      time.Now,
	}, nil
}

func newScaleClient(cfg *Config) (*scaleset.Client, error) {
	info := scaleset.SystemInfo{System: "silo-runner", Subsystem: "listener", Version: version, CommitSHA: "NA"}
	auth := cfg.GitHub.Auth
	if auth.TokenEnv != "" {
		return scaleset.NewClientWithPersonalAccessToken(scaleset.NewClientWithPersonalAccessTokenConfig{
			GitHubConfigURL:     cfg.GitHub.ConfigURL,
			PersonalAccessToken: os.Getenv(auth.TokenEnv),
			SystemInfo:          info,
		})
	}
	key, err := os.ReadFile(auth.App.PrivateKeyFile)
	if err != nil {
		return nil, fmt.Errorf("cannot read the GitHub App private key: %w", err)
	}
	return scaleset.NewClientWithGitHubApp(scaleset.ClientWithGitHubAppConfig{
		GitHubConfigURL: cfg.GitHub.ConfigURL,
		GitHubAppAuth:   scaleset.GitHubAppAuth{ClientID: auth.App.ClientID, InstallationID: auth.App.InstallationID, PrivateKey: string(key)},
		SystemInfo:      info,
	})
}

// CheckAccess runs the private-repository check alone.
func CheckAccess(ctx context.Context, cfg *Config, deps *Deps) error {
	target, err := ParseTarget(cfg.GitHub.ConfigURL)
	if err != nil {
		return err
	}
	return deps.Checker.CheckPrivate(ctx, target, cfg.GitHub.RunnerGroup)
}

// Run serves the configured scale sets until ctx ends.
func Run(ctx context.Context, cfg *Config, deps *Deps) error {
	logger := deps.Logger
	if deps.Now == nil {
		deps.Now = time.Now
	}
	report, err := ApplyHost(cfg, deps.Host)
	if err != nil {
		return err
	}
	if report.LinuxHost {
		logger.Info(report.Describe())
	}
	for _, warning := range SecretFileWarnings(cfg, envFile()) {
		logger.Warn(warning)
	}
	if err := CheckAccess(ctx, cfg, deps); err != nil {
		return err
	}
	lock, err := AcquireLock(cfg.StateDir)
	if err != nil {
		return err
	}
	defer lock.Release()
	ledger, err := OpenLedger(cfg.StateDir)
	if err != nil {
		return err
	}
	groupID := defaultRunnerGroupID
	if cfg.GitHub.RunnerGroup != "default" {
		group, err := deps.API.GetRunnerGroupByName(ctx, cfg.GitHub.RunnerGroup)
		if err != nil {
			return fmt.Errorf("cannot find the runner group %q: %w", cfg.GitHub.RunnerGroup, err)
		}
		groupID = group.ID
	}
	disk := DiskGuard{Path: cfg.Limits.DiskPath, MinGiB: cfg.Limits.MinFreeDiskGiB, Free: deps.Disk}
	gates := map[string]*sync.RWMutex{}
	recipes := map[string]string{}
	var sets []FleetSet
	owner, _ := os.Hostname()
	sessions := map[string]int{}
	for _, spec := range cfg.ScaleSets {
		set, err := ensureScaleSet(ctx, deps.API, groupID, spec)
		if err != nil {
			return err
		}
		recipes[spec.Recipe] = spec.OS
		start := cfg.Runner.LinuxStart
		if spec.OS == "macos" {
			start = cfg.Runner.MacosStart
		}
		sets = append(sets, FleetSet{Spec: spec, ScaleSetID: set.ID, Start: start})
		sessions[spec.Label] = set.ID
	}
	fleet := NewFleet(FleetOptions{
		Sets: sets, ProjectDir: cfg.Silo.ProjectDir, Linux: cfg.LinuxHost,
		ReadyTimeout: cfg.Limits.ReadyTimeout.Std(), StartAttempts: cfg.Limits.StartAttempts, IdleTimeout: cfg.Limits.IdleTimeout.Std(),
		TickEvery: tickEvery, Logger: logger, Now: deps.Now,
	}, deps.API, deps.Silo, disk, ledger, gates)
	fleet.Start()
	var wg sync.WaitGroup
	for _, spec := range cfg.ScaleSets {
		scaleSetID := sessions[spec.Label]
		loop := &ListenerLoop{
			Scaler: fleet.Scaler(spec.Label),
			OpenSession: func(ctx context.Context) (Session, error) {
				return deps.API.OpenSession(ctx, scaleSetID, owner)
			},
			NewListener: func(sess Session) (Runnable, error) {
				return listener.New(sess, listener.Config{ScaleSetID: scaleSetID, MaxRunners: spec.MaxRunners, Logger: logger.With("label", spec.Label).WithGroup("listener")})
			},
			Stall:  cfg.Limits.ListenerStall.Std(),
			Logger: logger.With("label", spec.Label),
		}
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = loop.Run(ctx)
			logger.Info("listener stopped", "label", spec.Label)
		}()
	}
	if !cfg.Refresh.Disabled {
		refresher := &Refresher{
			ProjectDir: cfg.Silo.ProjectDir, Recipes: recipes, Source: deps.Releases, Silo: deps.Silo, Reserve: fleet.Reserve,
			Gates: gates, AfterDays: cfg.Refresh.AfterDays, Now: deps.Now, Logger: logger,
		}
		wg.Add(1)
		go func() {
			defer wg.Done()
			refresher.Loop(ctx, cfg.Refresh.CheckEvery.Std())
		}()
	}
	logger.Info("silo-runner is serving", "scaleSets", len(cfg.ScaleSets))
	<-ctx.Done()
	wg.Wait()
	fleet.Shutdown()
	return nil
}

// ensureScaleSet finds or creates the scale set for a label, with updates disabled in the runner.
func ensureScaleSet(ctx context.Context, api ScaleAPI, groupID int, spec ScaleSetSpec) (*scaleset.RunnerScaleSet, error) {
	want := &scaleset.RunnerScaleSet{
		Name:          spec.Label,
		RunnerGroupID: groupID,
		Labels:        []scaleset.Label{{Name: spec.Label}},
		RunnerSetting: scaleset.RunnerSetting{DisableUpdate: true},
	}
	found, err := api.GetRunnerScaleSet(ctx, groupID, spec.Label)
	if err != nil {
		return nil, fmt.Errorf("cannot look up the scale set %q: %w", spec.Label, err)
	}
	if found == nil {
		created, err := api.CreateRunnerScaleSet(ctx, want)
		if err != nil {
			return nil, fmt.Errorf("cannot create the scale set %q: %w", spec.Label, err)
		}
		return created, nil
	}
	if !found.RunnerSetting.DisableUpdate {
		updated, err := api.UpdateRunnerScaleSet(ctx, found.ID, want)
		if err != nil {
			return nil, fmt.Errorf("cannot turn runner updates off for the scale set %q: %w", spec.Label, err)
		}
		return updated, nil
	}
	return found, nil
}

// SignalContext ends on SIGINT or SIGTERM.
func SignalContext() (context.Context, context.CancelFunc) {
	return signal.NotifyContext(context.Background(), os.Interrupt, terminateSignal)
}

// InstallRecipes writes the shipped runner recipes into the project directory. A recipe that is
// already there is kept, unless replace is set: then it is moved to <name>.yaml.bak (replacing an
// older .bak) and the shipped one is written, unless the two are identical.
func InstallRecipes(projectDir string, replace bool, out func(format string, args ...any)) error {
	dir := filepath.Join(projectDir, ".silo", "recipes")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	entries, err := shippedRecipes.ReadDir("recipes")
	if err != nil {
		return err
	}
	for _, entry := range entries {
		target := filepath.Join(dir, entry.Name())
		data, err := shippedRecipes.ReadFile("recipes/" + entry.Name())
		if err != nil {
			return err
		}
		if existing, err := os.ReadFile(target); err == nil {
			switch {
			case !replace:
				out("kept the existing %s\n", target)
				continue
			case bytes.Equal(existing, data):
				out("%s is already the shipped recipe\n", target)
				continue
			}
			if err := os.Rename(target, target+".bak"); err != nil {
				return err
			}
			out("moved the existing %s to %s.bak\n", target, target)
		}
		if err := os.WriteFile(target, data, 0o644); err != nil {
			return err
		}
		out("wrote %s\n", target)
	}
	return nil
}
