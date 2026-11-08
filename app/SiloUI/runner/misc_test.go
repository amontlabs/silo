package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"maps"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/actions/scaleset"
	"github.com/actions/scaleset/listener"
)

func quiet() *slog.Logger { return slog.New(slog.NewTextHandler(io.Discard, nil)) }

// MARK: capacity

func TestMacCapacityCountsEveryRunningMacVMAndForksInProgress(t *testing.T) {
	listed := []Computer{
		{Name: "a", Kind: "macos", State: "running"},
		{Name: "b", Kind: "macos", State: "stopped"},
		{Name: "c", Kind: "linux", State: "running"},
	}
	capacity := Capacity{}
	if err := capacity.Allows("macos", 8, listed, nil); err != nil {
		t.Fatal(err)
	}
	// One running + one fork in progress = the limit of two.
	pending := []Demand{{OS: "macos", MemoryGiB: 8}}
	if err := capacity.Allows("macos", 8, listed, pending); !errors.Is(err, errNoCapacity) {
		t.Fatalf("err = %v", err)
	}
	// A VM that is starting or stopping still holds its slot.
	listed[1].State = "stopping"
	if err := capacity.Allows("macos", 8, listed, nil); !errors.Is(err, errNoCapacity) {
		t.Fatalf("a stopping VM holds a slot: %v", err)
	}
}

func TestLinuxCapacityHonoursTheCountAndMemoryBudget(t *testing.T) {
	listed := []Computer{{Name: "l1", Kind: "linux", State: "running", MemoryGiB: 8}}
	capacity := Capacity{Linux: LinuxBudget{MemoryGiB: 20, Count: 3}}
	pending := []Demand{{OS: "linux", MemoryGiB: 8}}
	if err := capacity.Allows("linux", 4, listed, pending); err != nil {
		t.Fatal(err)
	}
	if err := capacity.Allows("linux", 8, listed, pending); !errors.Is(err, errNoCapacity) {
		t.Fatalf("16 GiB in use plus 8 more exceeds 20 GiB: %v", err)
	}
	if err := (Capacity{Linux: LinuxBudget{Count: 1}}).Allows("linux", 1, listed, nil); !errors.Is(err, errNoCapacity) {
		t.Fatalf("count: %v", err)
	}
}

func TestRecipeMemoryComesFromTheRecipeFile(t *testing.T) {
	dir := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, ".silo", "recipes"), 0o755); err != nil {
		t.Fatal(err)
	}
	write := func(name, text string) {
		if err := os.WriteFile(filepath.Join(dir, ".silo", "recipes", name+".yaml"), []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("big", "os: linux\nmemory: 16\nsetup:\n  - echo hi\n")
	write("bare", "os: linux\n")
	write("broken", "memory: [1\n")
	for recipe, want := range map[string]int{"big": 16, "bare": 4, "broken": 4, "missing": 4} {
		if got := RecipeMemoryGiB(dir, recipe, "linux"); got != want {
			t.Errorf("%s: %d, want %d", recipe, got, want)
		}
	}
	if got := RecipeMemoryGiB(dir, "missing", "macos"); got != 8 {
		t.Errorf("a macOS recipe without memory defaults to 8: %d", got)
	}
	for _, name := range []string{"silo-linux", "silo-macos"} {
		raw, err := shippedRecipes.ReadFile("recipes/" + name + ".yaml")
		if err != nil {
			t.Fatal(err)
		}
		write(name, string(raw))
		if got := RecipeMemoryGiB(dir, name, "linux"); got != 8 {
			t.Errorf("%s: %d", name, got)
		}
	}
}

// MARK: public repositories

func githubServer(t *testing.T, routes map[string]string) *RESTChecker {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer tok" {
			http.Error(w, "no", http.StatusUnauthorized)
			return
		}
		body, ok := routes[r.URL.Path]
		if !ok {
			http.NotFound(w, r)
			return
		}
		fmt.Fprint(w, body)
	}))
	t.Cleanup(server.Close)
	return &RESTChecker{APIBase: server.URL, Token: func(context.Context) (string, error) { return "tok", nil }}
}

func TestAPublicRepositoryIsRefusedWithGitHubsReason(t *testing.T) {
	checker := githubServer(t, map[string]string{"/repos/o/pub": `{"private": false, "visibility": "public"}`, "/repos/o/priv": `{"private": true, "visibility": "private"}`})
	err := checker.CheckPrivate(context.Background(), Target{Owner: "o", Repo: "pub"}, "default")
	if err == nil || !strings.Contains(err.Error(), "should almost never be used for public repositories") {
		t.Fatalf("err = %v", err)
	}
	if err := checker.CheckPrivate(context.Background(), Target{Owner: "o", Repo: "priv"}, "default"); err != nil {
		t.Fatal(err)
	}
	if err := checker.CheckPrivate(context.Background(), Target{Owner: "o", Repo: "missing"}, "default"); err == nil {
		t.Fatal("a repository that cannot be read cannot be shown private")
	}
}

func TestAnOrgRunnerGroupThatAllowsPublicRepositoriesIsRefused(t *testing.T) {
	checker := githubServer(t, map[string]string{"/orgs/acme/actions/runner-groups": `{"runner_groups":[
		{"name":"Default","allows_public_repositories":true},
		{"name":"safe","allows_public_repositories":false},
		{"name":"unknown"}]}`})
	ctx := context.Background()
	err := checker.CheckPrivate(ctx, Target{Owner: "acme"}, "default")
	if err == nil || !strings.Contains(err.Error(), "allows public repositories") || !strings.Contains(err.Error(), "almost never") {
		t.Fatalf("err = %v", err)
	}
	if err := checker.CheckPrivate(ctx, Target{Owner: "acme"}, "safe"); err != nil {
		t.Fatal(err)
	}
	if err := checker.CheckPrivate(ctx, Target{Owner: "acme"}, "unknown"); err == nil {
		t.Fatal("a group that does not report the field is not shown safe")
	}
	if err := checker.CheckPrivate(ctx, Target{Owner: "acme"}, "nope"); err == nil {
		t.Fatal("missing group")
	}
}

type staticChecker struct{ err error }

func (s staticChecker) CheckPrivate(context.Context, Target, string) error { return s.err }

func TestRunRefusesToStartBeforeTouchingGitHubOrSilo(t *testing.T) {
	cfg := testConfig(t)
	silo := newSilo(&events{})
	deps := &Deps{Silo: silo, Checker: staticChecker{errors.New("o/r is a public repository")}, Logger: quiet()}
	err := Run(context.Background(), cfg, deps)
	if err == nil || !strings.Contains(err.Error(), "public repository") {
		t.Fatalf("err = %v", err)
	}
}

// MARK: config

func testConfig(t *testing.T) *Config {
	t.Helper()
	dir := t.TempDir()
	cfg, err := ParseConfig([]byte(fmt.Sprintf(`
github:
  configUrl: https://github.com/o/r
  auth: {tokenEnv: GH_TOKEN}
stateDir: %s/state
silo: {projectDir: %s/project}
scaleSets:
  - {label: silo-linux, recipe: silo-linux, os: linux, maxRunners: 2}
`, dir, dir)), "test.yaml")
	if err != nil {
		t.Fatal(err)
	}
	return cfg
}

func TestConfigAcceptsYAMLAndJSONAndAppliesDefaults(t *testing.T) {
	yamlCfg := `
github:
  configUrl: https://github.com/acme
  auth:
    app: {clientId: Iv1.x, installationId: 5, privateKeyFile: /k.pem}
scaleSets:
  - {label: silo-mac, recipe: silo-macos, os: macos, maxRunners: 2, keepFailed: {count: 3, days: 2}}
`
	cfg, err := ParseConfig([]byte(yamlCfg), "c.yaml")
	if err != nil {
		t.Fatal(err)
	}
	set := cfg.ScaleSets[0]
	if set.KeepFailed.Limit() != 3 || set.KeepFailed.Days != 2 || cfg.GitHub.RunnerGroup != "default" || cfg.Limits.MinFreeDiskGiB != 20 {
		t.Fatalf("%+v", cfg)
	}
	if target, _ := ParseTarget(cfg.GitHub.ConfigURL); !target.IsOrg() {
		t.Fatal("an organization URL")
	}
	jsonCfg := `{"github":{"configUrl":"https://github.com/o/r","auth":{"tokenEnv":"T"}},"scaleSets":[{"label":"silo-linux","recipe":"r"}]}`
	if _, err := ParseConfig([]byte(jsonCfg), "c.json"); err != nil {
		t.Fatal(err)
	}
}

func TestConfigErrors(t *testing.T) {
	base := func(extra string) string {
		return "github: {configUrl: 'https://github.com/o/r', auth: {tokenEnv: T}}\n" + extra
	}
	for name, text := range map[string]string{
		"unknown key":       base("scaleSets: [{label: a, recipe: r}]\nbogus: 1\n"),
		"no scale sets":     base(""),
		"duplicate label":   base("scaleSets: [{label: a, recipe: r}, {label: a, recipe: s}]\n"),
		"three macOS":       base("scaleSets: [{label: a, recipe: r, os: macos, maxRunners: 3}]\n"),
		"bad os":            base("scaleSets: [{label: a, recipe: r, os: windows}]\n"),
		"label with space":  base("scaleSets: [{label: 'a b', recipe: r}]\n"),
		"both auths":        "github: {configUrl: 'https://github.com/o/r', auth: {tokenEnv: T, app: {clientId: a, installationId: 1, privateKeyFile: k}}}\nscaleSets: [{label: a, recipe: r}]\n",
		"no auth":           "github: {configUrl: 'https://github.com/o/r'}\nscaleSets: [{label: a, recipe: r}]\n",
		"enterprise":        "github: {configUrl: 'https://github.com/enterprises/e', auth: {tokenEnv: T}}\nscaleSets: [{label: a, recipe: r}]\n",
		"late refresh days": base("runnerRefresh: {afterDays: 29}\nscaleSets: [{label: a, recipe: r}]\n"),
	} {
		if _, err := ParseConfig([]byte(text), "c.yaml"); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

// MARK: watchdog

type stallSession struct{ closed atomic.Int32 }

func (s *stallSession) GetMessage(context.Context, int, int) (*scaleset.RunnerScaleSetMessage, error) {
	return nil, nil
}
func (s *stallSession) DeleteMessage(context.Context, int) error { return nil }
func (s *stallSession) AcquireJobs(context.Context, []int64) ([]int64, error) {
	return nil, nil
}
func (s *stallSession) Session() scaleset.RunnerScaleSetSession {
	return scaleset.RunnerScaleSetSession{}
}
func (s *stallSession) Close(context.Context) error { s.closed.Add(1); return nil }

type blockingListener struct {
	runs   *atomic.Int32
	scaler *Scaler
	// answerAfter makes the nth run poll, so the watchdog stays quiet.
	answerAfter int32
}

func (b blockingListener) Run(ctx context.Context, scaler listener.Scaler) error {
	n := b.runs.Add(1)
	if n >= b.answerAfter {
		ticker := time.NewTicker(10 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-ticker.C:
				_ = scaler.Scale(ctx, nil)
			}
		}
	}
	<-ctx.Done()
	return ctx.Err()
}

func TestTheWatchdogRestartsASessionWhoseListenerStopsPolling(t *testing.T) {
	h := newHarness(t, harnessOptions{})
	var runs atomic.Int32
	var opened atomic.Int32
	var session stallSession
	loop := &ListenerLoop{
		Scaler: h.scaler,
		OpenSession: func(context.Context) (Session, error) {
			opened.Add(1)
			return &session, nil
		},
		NewListener: func(Session) (Runnable, error) {
			return blockingListener{runs: &runs, scaler: h.scaler, answerAfter: 3}, nil
		},
		Stall: 80 * time.Millisecond, Check: 10 * time.Millisecond, Backoff: time.Millisecond, Logger: quiet(),
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- loop.Run(ctx) }()
	deadline := time.Now().Add(5 * time.Second)
	for runs.Load() < 3 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if runs.Load() < 3 {
		t.Fatalf("the session was not restarted: runs %d", runs.Load())
	}
	cancel()
	<-done
	if loop.Restarts < 2 {
		t.Fatalf("restarts = %d", loop.Restarts)
	}
	if opened.Load() < 3 || session.closed.Load() < 3 {
		t.Fatalf("opened %d closed %d: each stalled session must be closed", opened.Load(), session.closed.Load())
	}
}

// MARK: runner version

func recipeText(version, sha string) string {
	return "os: linux\nmemory: 8\nsetup:\n  - |\n    RUNNER_VERSION=" + version + "\n    RUNNER_SHA256=" + sha + "\n    curl https://x/actions-runner-linux-arm64-${RUNNER_VERSION}.tar.gz\n"
}

type fixedRelease Release

func (f fixedRelease) Latest(context.Context) (Release, error) { return Release(f), nil }

// capacityOf checks capacity against the fake's computers, with nothing pending.
func capacityOf(silo *fakeSilo, linux LinuxBudget) func(context.Context, string, int) (func(), error) {
	return func(ctx context.Context, os string, memoryGiB int) (func(), error) {
		computers, err := silo.List(ctx)
		if err != nil {
			return nil, err
		}
		return func() {}, Capacity{Linux: linux}.Allows(os, memoryGiB, computers, nil)
	}
}

func refresher(t *testing.T, text string, release Release, now time.Time) (*Refresher, *fakeSilo, string) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, ".silo", "recipes", "silo-linux.yaml")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
	silo := newSilo(&events{})
	return &Refresher{ProjectDir: dir, Recipes: map[string]string{"silo-linux": "linux"}, Source: fixedRelease(release), Silo: silo,
		Reserve: capacityOf(silo, LinuxBudget{}), AfterDays: 3, Now: func() time.Time { return now }, Logger: quiet()}, silo, path
}

func TestARecipeMovesToANewRunnerOnceTheReleaseIsOldEnough(t *testing.T) {
	sha := strings.Repeat("b", 64)
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{"actions-runner-linux-arm64-2.339.0.tar.gz": sha}}
	r, silo, path := refresher(t, recipeText("2.338.2", strings.Repeat("a", 64)), release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	if err := r.CheckOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	got, _ := os.ReadFile(path)
	if !strings.Contains(string(got), "RUNNER_VERSION=2.339.0") || !strings.Contains(string(got), sha) {
		t.Fatalf("recipe = %s", got)
	}
	if len(silo.built) != 1 || silo.built[0] != "silo-linux" {
		t.Fatalf("built = %v", silo.built)
	}
}

func TestARecipeWaitsForAFreshReleaseAndIgnoresACurrentOne(t *testing.T) {
	text := recipeText("2.338.2", strings.Repeat("a", 64))
	now := time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC)
	fresh := Release{Version: "2.339.0", Published: now.Add(-24 * time.Hour)}
	r, silo, _ := refresher(t, text, fresh, now)
	if err := r.CheckOnce(context.Background()); err != nil || len(silo.built) != 0 {
		t.Fatalf("err %v built %v", err, silo.built)
	}
	current := Release{Version: "2.338.2", Published: now.Add(-100 * 24 * time.Hour)}
	r, silo, _ = refresher(t, text, current, now)
	if err := r.CheckOnce(context.Background()); err != nil || len(silo.built) != 0 {
		t.Fatalf("err %v built %v", err, silo.built)
	}
}

func TestAFailedRebuildRestoresTheOldRecipeAndNamesTheDeadline(t *testing.T) {
	old := recipeText("2.338.2", strings.Repeat("a", 64))
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{"actions-runner-linux-arm64-2.339.0.tar.gz": strings.Repeat("b", 64)}}
	r, silo, path := refresher(t, old, release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	silo.buildErr = errors.New("download failed")
	err := r.CheckOnce(context.Background())
	if err == nil || !strings.Contains(err.Error(), "2026-12-01") {
		t.Fatalf("err = %v", err)
	}
	if got, _ := os.ReadFile(path); string(got) != old {
		t.Fatalf("the old recipe was not restored: %s", got)
	}
}

func TestNoDigestMeansNoRefresh(t *testing.T) {
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{}}
	r, silo, _ := refresher(t, recipeText("2.338.2", strings.Repeat("a", 64)), release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	if err := r.CheckOnce(context.Background()); err == nil || len(silo.built) != 0 {
		t.Fatalf("err %v built %v", err, silo.built)
	}
}

func TestShippedRecipesCarryAParseablePin(t *testing.T) {
	want := map[string]map[string]string{
		"silo-linux": {
			"arm64": "670adb2f268fe21dcbb58b3db0e0aa9d5b3bddfe2c651481336e561ff86de945",
			"x64":   "baebf204307bac6193480852732f0aba2ae97b6c5597c244f4d5558390147c40",
		},
		"silo-macos": {"arm64": "25a065af409ffa317ab4d5a3b614dd15fc9c40f2172bc1b35a1b1258e919a388"},
	}
	for name, digests := range want {
		data, err := shippedRecipes.ReadFile("recipes/" + name + ".yaml")
		if err != nil {
			t.Fatal(err)
		}
		pin, err := ParsePin(string(data))
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if pin.Version != "2.338.2" || !maps.Equal(pin.SHA256, digests) {
			t.Fatalf("%s: %+v", name, pin)
		}
	}
	if compareVersions("2.338.2", "2.339.0") >= 0 || compareVersions("2.10.0", "2.9.9") <= 0 || compareVersions("1.0", "1.0.0") != 0 {
		t.Fatal("compareVersions")
	}
}

func TestTheLinuxRecipePicksItsArchitectureAndChecksThatDigest(t *testing.T) {
	data, err := shippedRecipes.ReadFile("recipes/silo-linux.yaml")
	if err != nil {
		t.Fatal(err)
	}
	recipe := string(data)
	for _, want := range []string{
		`aarch64|arm64) arch=arm64; RUNNER_SHA256=$RUNNER_SHA256_ARM64`,
		`x86_64|amd64) arch=x64; RUNNER_SHA256=$RUNNER_SHA256_X64`,
		`actions-runner-linux-${arch}-${RUNNER_VERSION}.tar.gz`,
		`sha256sum -c -`,
	} {
		if !strings.Contains(recipe, want) {
			t.Fatalf("the recipe lacks %q", want)
		}
	}
	if strings.Contains(recipe, "linux-arm64-${RUNNER_VERSION}") {
		t.Fatal("the recipe still downloads one fixed architecture")
	}
}

func TestARefreshPinsEveryArchitectureToItsOwnAsset(t *testing.T) {
	arm, x64 := strings.Repeat("c", 64), strings.Repeat("d", 64)
	text := "setup:\n  - |\n    RUNNER_VERSION=2.338.2\n    RUNNER_SHA256_ARM64=" + strings.Repeat("a", 64) + "\n    RUNNER_SHA256_X64=" + strings.Repeat("b", 64) +
		"\n    RUNNER_SHA256=$RUNNER_SHA256_ARM64\n    curl https://x/actions-runner-linux-${arch}-${RUNNER_VERSION}.tar.gz\n"
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{
		"actions-runner-linux-arm64-2.339.0.tar.gz": arm, "actions-runner-linux-x64-2.339.0.tar.gz": x64,
		"actions-runner-linux-arm-2.339.0.tar.gz": strings.Repeat("e", 64),
	}}
	r, silo, path := refresher(t, text, release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	if err := r.CheckOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	got, _ := os.ReadFile(path)
	pin, err := ParsePin(string(got))
	if err != nil {
		t.Fatal(err)
	}
	if pin.Version != "2.339.0" || pin.SHA256["arm64"] != arm || pin.SHA256["x64"] != x64 || !strings.Contains(string(got), "RUNNER_SHA256=$RUNNER_SHA256_ARM64") {
		t.Fatalf("recipe = %s", got)
	}
	if len(silo.built) != 1 {
		t.Fatalf("built = %v", silo.built)
	}
	// One architecture without a published digest leaves the recipe alone.
	delete(release.Digests, "actions-runner-linux-x64-2.339.0.tar.gz")
	r, silo, path = refresher(t, text, release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	if err := r.CheckOnce(context.Background()); err == nil || !strings.Contains(err.Error(), "linux-x64-2.339.0") || len(silo.built) != 0 {
		t.Fatalf("err %v built %v", err, silo.built)
	}
	if again, _ := os.ReadFile(path); string(again) != text {
		t.Fatal("the recipe changed although one digest was missing")
	}
}

func TestARecipeWhoseDigestsDoNotMatchItsDownloadIsRefused(t *testing.T) {
	sha := strings.Repeat("a", 64)
	for name, text := range map[string]string{
		"suffix with a fixed download": "RUNNER_VERSION=1.2.3\nRUNNER_SHA256_X64=" + sha + "\ncurl actions-runner-linux-arm64-${RUNNER_VERSION}.tar.gz",
		"one digest for ${arch}":       "RUNNER_VERSION=1.2.3\nRUNNER_SHA256_X64=" + sha + "\ncurl actions-runner-linux-${arch}-${RUNNER_VERSION}.tar.gz",
		"bare digest for ${arch}":      "RUNNER_VERSION=1.2.3\nRUNNER_SHA256=" + sha + "\ncurl actions-runner-linux-${arch}-${RUNNER_VERSION}.tar.gz",
		"two bare digests":             "RUNNER_VERSION=1.2.3\nRUNNER_SHA256=" + sha + "\nRUNNER_SHA256=" + sha + "\ncurl actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz",
	} {
		if _, err := ParsePin(text); err == nil {
			t.Fatalf("%s: accepted", name)
		}
	}
	if pin, err := ParsePin("RUNNER_VERSION=1.2.3\nRUNNER_SHA256=" + sha + "\ncurl actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz"); err != nil || pin.SHA256["x64"] != sha {
		t.Fatalf("pin %+v err %v", pin, err)
	}
}

// MARK: the silo command line

func TestTheJITConfigReachesSiloOnStandardInputAndNeverInArgv(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "silo")
	record := filepath.Join(dir, "record")
	body := `#!/bin/sh
{ printf 'ARGV:'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> ` + record + `
case " $* " in *" exec "*)
  IFS= read -r line
  echo "STDIN:$line" >> ` + record + `
  echo "Listening for Jobs" ;;
esac
`
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	cli := &CLI{Path: script, Dir: dir, Device: "mini"}
	proc, err := cli.StartRunner(context.Background(), "ci-silo-linux-1", "SECRET-JIT-VALUE", []string{"/opt/actions-runner/silo-run-jit"})
	if err != nil {
		t.Fatal(err)
	}
	if err := waitReady(context.Background(), proc, 5*time.Second); err != nil {
		t.Fatal(err)
	}
	for range proc.Lines() {
	}
	if err := proc.Wait(); err != nil {
		t.Fatal(err)
	}
	raw, _ := os.ReadFile(record)
	text := string(raw)
	if !strings.Contains(text, "STDIN:SECRET-JIT-VALUE") {
		t.Fatalf("stdin was not delivered: %s", text)
	}
	argv := text[:strings.Index(text, "STDIN:")]
	if strings.Contains(argv, "SECRET") || strings.Contains(argv, "JIT-VALUE") {
		t.Fatalf("the JIT value is in argv: %s", argv)
	}
	if !strings.Contains(argv, "[--device] [mini] [exec] [ci-silo-linux-1] [--stream] [--stdin] [--] [/opt/actions-runner/silo-run-jit]") {
		t.Fatalf("argv = %s", argv)
	}
}

func TestTheCLIParsesListAndPassesTheCommandsItShouldUse(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "silo")
	record := filepath.Join(dir, "record")
	body := `#!/bin/sh
echo "$*" >> ` + record + `
if [ "$1" = list ]; then
  echo '{"computers":[{"id":"i1","name":"a","kind":"macos","state":"running","memoryGiB":8}]}'
fi
if [ "$1" = fork ]; then
  echo '{"cpus":4,"id":"i2","kind":"linux","memoryGiB":8,"name":"ci-x-1","ports":[],"source":"silo-linux","state":"stopped"}'
fi
`
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	cli := &CLI{Path: script, Dir: dir}
	ctx := context.Background()
	computers, err := cli.List(ctx)
	if err != nil || len(computers) != 1 || !computers[0].Occupies() || computers[0].ID != "i1" {
		t.Fatalf("%v %v", computers, err)
	}
	if id, err := cli.Fork(ctx, "ci-x-1", "silo-linux"); err != nil || id != "i2" {
		t.Fatalf("fork id %q err %v", id, err)
	}
	_ = cli.Start(ctx, "i2")
	_ = cli.Stop(ctx, "i2")
	_ = cli.BuildRecipe(ctx, "silo-linux")
	raw, _ := os.ReadFile(record)
	for _, want := range []string{"list --all --json", "fork ci-x-1 --recipe silo-linux --json", "start i2 --json", "stop i2 --json", "recipe build silo-linux --json"} {
		if !strings.Contains(string(raw), want+"\n") {
			t.Errorf("missing %q in\n%s", want, raw)
		}
	}
}

// `silo drop` does not stop a running computer and the engine refuses to delete one, so the
// helper stops it first; and it only ever drops a computer a list shows, by id.
func TestDropStopsARunningComputerFirstAndGoesByID(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "silo")
	record := filepath.Join(dir, "record")
	body := `#!/bin/sh
echo "$*" >> ` + record + `
if [ "$1" = list ]; then
  echo '{"computers":[{"id":"i1","name":"a","kind":"linux","state":"running","memoryGiB":8},{"id":"i3","name":"b","kind":"linux","state":"stopped","memoryGiB":8}]}'
fi
`
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	cli := &CLI{Path: script, Dir: dir}
	ctx := context.Background()
	if err := cli.Drop(ctx, "i1"); err != nil {
		t.Fatal(err)
	}
	if err := cli.Drop(ctx, "i3"); err != nil {
		t.Fatal(err)
	}
	if err := cli.Drop(ctx, "b"); err == nil {
		t.Fatal("a name is not an id: only a listed id is dropped")
	}
	raw, _ := os.ReadFile(record)
	want := "list --all --json\nstop i1 --json\ndrop i1 --json\nlist --all --json\ndrop i3 --json\nlist --all --json\n"
	if string(raw) != want {
		t.Fatalf("calls:\n%s\nwant:\n%s", raw, want)
	}
}

// A computer a list shows as stopped is already stopped: the engine refuses to stop it again, so a
// stop that worked but was not recorded must not be retried into that error forever.
func TestStopTreatsAnAlreadyStoppedComputerAsDone(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "silo")
	record := filepath.Join(dir, "record")
	body := `#!/bin/sh
echo "$*" >> ` + record + `
if [ "$1" = list ]; then
  echo '{"computers":[{"id":"i1","name":"a","kind":"macos","state":"running","memoryGiB":8},{"id":"i2","name":"b","kind":"macos","state":"stopped","memoryGiB":8}]}'
fi
if [ "$1" = stop ] && [ "$2" = i2 ]; then
  echo "This computer isn't running." >&2
  exit 1
fi
`
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	cli := &CLI{Path: script, Dir: dir}
	ctx := context.Background()
	if err := cli.Stop(ctx, "i2"); err != nil {
		t.Fatalf("an already stopped computer is stopped: %v", err)
	}
	if err := cli.Stop(ctx, "i1"); err != nil {
		t.Fatal(err)
	}
	raw, _ := os.ReadFile(record)
	want := "list --all --json\nlist --all --json\nstop i1 --json\n"
	if string(raw) != want {
		t.Fatalf("calls:\n%s\nwant:\n%s", raw, want)
	}
}

// MARK: kept store and lock

func TestLedgerPersistsAndTheLockIsExclusive(t *testing.T) {
	dir := t.TempDir()
	ledger, _ := OpenLedger(dir)
	if err := ledger.Put(Entry{Name: "ci-a-1", Label: "a", Phase: PhaseForking, At: time.Unix(5, 0)}); err != nil {
		t.Fatal(err)
	}
	if err := ledger.Put(Entry{Name: "ci-a-1", ID: "id-1", Label: "a", Phase: PhaseKept, Result: "failed", At: time.Unix(9, 0), Request: 4}); err != nil {
		t.Fatal(err)
	}
	if err := ledger.Put(Entry{Name: "ci-a-2", Label: "a", Phase: PhaseForking, At: time.Unix(7, 0)}); err != nil {
		t.Fatal(err)
	}
	if err := ledger.Remove("ci-a-2"); err != nil {
		t.Fatal(err)
	}
	again, err := OpenLedger(dir)
	entry, ok := again.Get("ci-a-1")
	if err != nil || !ok || entry.ID != "id-1" || entry.Phase != PhaseKept || entry.Request != 4 || !entry.At.Equal(time.Unix(9, 0)) || len(again.All()) != 1 {
		t.Fatalf("%v %+v", err, entry)
	}
	if err := os.WriteFile(filepath.Join(dir, "computers.json"), []byte(`[{"name":"x","label":"a","phase":"sideways"}]`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenLedger(dir); err == nil {
		t.Fatal("an unknown phase must not be guessed at")
	}
	lock, err := AcquireLock(dir)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := AcquireLock(dir); err == nil {
		t.Fatal("a second helper must not start")
	}
	lock.Release()
	second, err := AcquireLock(dir)
	if err != nil {
		t.Fatal(err)
	}
	second.Release()
}

// A crash must leave the old ledger or the new one: the file is flushed before it replaces the
// old one, and the directory after, so the rename survives a power cut.
func TestLedgerSavesFlushTheFileAndItsDirectory(t *testing.T) {
	dir := t.TempDir()
	ledger, _ := OpenLedger(dir)
	final := filepath.Join(dir, "computers.json")
	var flushed []string
	var finalAtFileFlush, finalAtDirFlush string
	ledger.sync = func(f *os.File) error {
		flushed = append(flushed, f.Name())
		raw, _ := os.ReadFile(final)
		if len(flushed) == 1 {
			finalAtFileFlush = string(raw)
		} else {
			finalAtDirFlush = string(raw)
		}
		return f.Sync()
	}
	if err := ledger.Put(Entry{Name: "ci-a-1", Label: "a", Phase: PhaseForking}); err != nil {
		t.Fatal(err)
	}
	if len(flushed) != 2 || flushed[0] != final+".tmp" || flushed[1] != dir {
		t.Fatalf("flushed %v", flushed)
	}
	if finalAtFileFlush != "" || !strings.Contains(finalAtDirFlush, "ci-a-1") {
		t.Fatalf("the file must be flushed before it replaces the ledger and the directory after: %q then %q", finalAtFileFlush, finalAtDirFlush)
	}
}

func TestInstallRecipesDoesNotReplaceAnExistingRecipe(t *testing.T) {
	dir := t.TempDir()
	existing := filepath.Join(dir, ".silo", "recipes", "silo-linux.yaml")
	_ = os.MkdirAll(filepath.Dir(existing), 0o755)
	_ = os.WriteFile(existing, []byte("mine"), 0o644)
	if err := InstallRecipes(dir, func(string, ...any) {}); err != nil {
		t.Fatal(err)
	}
	if got, _ := os.ReadFile(existing); string(got) != "mine" {
		t.Fatal("replaced")
	}
	if _, err := os.Stat(filepath.Join(dir, ".silo", "recipes", "silo-macos.yaml")); err != nil {
		t.Fatal(err)
	}
}

// ensureScaleSet

type fakeScaleAPI struct {
	ScaleAPI
	found   *scaleset.RunnerScaleSet
	created *scaleset.RunnerScaleSet
	updated bool
}

func (f *fakeScaleAPI) GetRunnerScaleSet(context.Context, int, string) (*scaleset.RunnerScaleSet, error) {
	return f.found, nil
}
func (f *fakeScaleAPI) CreateRunnerScaleSet(_ context.Context, s *scaleset.RunnerScaleSet) (*scaleset.RunnerScaleSet, error) {
	f.created = s
	s.ID = 9
	return s, nil
}
func (f *fakeScaleAPI) UpdateRunnerScaleSet(_ context.Context, id int, s *scaleset.RunnerScaleSet) (*scaleset.RunnerScaleSet, error) {
	f.updated = true
	s.ID = id
	return s, nil
}

func TestScaleSetsAreCreatedWithTheLabelAndRunnerUpdatesOff(t *testing.T) {
	spec := ScaleSetSpec{Label: "silo-linux", Recipe: "r"}
	api := &fakeScaleAPI{}
	set, err := ensureScaleSet(context.Background(), api, 1, spec)
	if err != nil || set.ID != 9 || !api.created.RunnerSetting.DisableUpdate || api.created.Labels[0].Name != "silo-linux" {
		t.Fatalf("%+v %v", api.created, err)
	}
	api = &fakeScaleAPI{found: &scaleset.RunnerScaleSet{ID: 4}}
	if _, err := ensureScaleSet(context.Background(), api, 1, spec); err != nil || !api.updated {
		t.Fatal("an existing scale set that lets the runner update itself must be switched off")
	}
	api = &fakeScaleAPI{found: &scaleset.RunnerScaleSet{ID: 4, RunnerSetting: scaleset.RunnerSetting{DisableUpdate: true}}}
	if _, err := ensureScaleSet(context.Background(), api, 1, spec); err != nil || api.updated {
		t.Fatal("no update needed")
	}
}

func TestTheExampleConfigurationIsValid(t *testing.T) {
	data, err := os.ReadFile("examples/config.yaml")
	if err != nil {
		t.Fatal(err)
	}
	cfg, err := ParseConfig(data, "examples/config.yaml")
	if err != nil {
		t.Fatal(err)
	}
	home, _ := os.UserHomeDir()
	if cfg.Silo.ProjectDir != filepath.Join(home, ".silo-runner", "project") || len(cfg.ScaleSets) != 2 {
		t.Fatalf("%+v", cfg.Silo)
	}
}

func TestKeepFailedCountDefaultsPerOperatingSystemAndZeroMeansNone(t *testing.T) {
	cfg, err := ParseConfig([]byte(`
github: {configUrl: 'https://github.com/o/r', auth: {tokenEnv: T}}
scaleSets:
  - {label: l, recipe: r}
  - {label: m, recipe: s, os: macos}
  - {label: z, recipe: t, keepFailed: {count: 0}}
`), "c.yaml")
	if err != nil {
		t.Fatal(err)
	}
	got := []int{cfg.ScaleSets[0].KeepFailed.Limit(), cfg.ScaleSets[1].KeepFailed.Limit(), cfg.ScaleSets[2].KeepFailed.Limit()}
	if got[0] != 3 || got[1] != 1 || got[2] != 0 {
		t.Fatalf("limits = %v", got)
	}
}

func TestARecipeRebuildNeedsTheRecipesMemory(t *testing.T) {
	release := Release{Version: "2.339.0", Published: time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC), Digests: map[string]string{"actions-runner-linux-arm64-2.339.0.tar.gz": strings.Repeat("b", 64)}}
	old := recipeText("2.338.2", strings.Repeat("a", 64))
	r, silo, path := refresher(t, old, release, time.Date(2026, 11, 8, 0, 0, 0, 0, time.UTC))
	silo.add(Computer{ID: "x", Name: "busy", Kind: "linux", State: "running", MemoryGiB: 8})
	r.Reserve = capacityOf(silo, LinuxBudget{MemoryGiB: 12})
	if err := r.CheckOnce(context.Background()); err == nil || !errors.Is(err, errNoCapacity) {
		t.Fatalf("the rebuild needs 8 GiB and only 4 are left: %v", err)
	}
	if got, _ := os.ReadFile(path); string(got) != old || len(silo.built) != 0 {
		t.Fatalf("nothing may change without room: built %v", silo.built)
	}
}

// The engine's refusal of a taken name reaches the helper as text on standard error, so the
// command line client marks that one failure and no other.
func TestTheCLIMarksAForkRefusedForATakenName(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "silo")
	body := `#!/bin/sh
case "$2" in
  taken) echo "silo: Computer names must be unique." >&2; exit 1 ;;
  *) echo "silo: the engine went away" >&2; exit 1 ;;
esac
`
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	cli := &CLI{Path: script, Dir: dir}
	if _, err := cli.Fork(context.Background(), "taken", "silo-linux"); !errors.Is(err, errNameTaken) {
		t.Fatalf("err = %v", err)
	}
	if _, err := cli.Fork(context.Background(), "other", "silo-linux"); err == nil || errors.Is(err, errNameTaken) {
		t.Fatalf("err = %v", err)
	}
}
