package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
)

var (
	versionLine = regexp.MustCompile(`(?m)^(\s*RUNNER_VERSION=)(\d+\.\d+\.\d+)$`)
	// shaLine matches RUNNER_SHA256= (for the one architecture the download names) and
	// RUNNER_SHA256_ARM64= or RUNNER_SHA256_X64= (for a download that picks its architecture).
	shaLine     = regexp.MustCompile(`(?m)^(\s*RUNNER_SHA256(?:_(ARM64|X64))?=)([0-9a-f]{64})$`)
	archiveName = regexp.MustCompile(`actions-runner-(linux|osx)-(arm64|x64|\$\{arch\})-\$\{RUNNER_VERSION\}\.tar\.gz`)
)

// chosenArch is the download's architecture when the recipe picks it at build time.
const chosenArch = "${arch}"

// Pin is the runner release a recipe installs.
type Pin struct {
	Version string
	// OS names the release assets: linux or osx.
	OS string
	// SHA256 maps an architecture (arm64 or x64) to the digest of that architecture's asset. A
	// recipe for one architecture has one entry; the Linux recipe, which runs on a Mac (arm64)
	// and on Linux hosts of either architecture, has both.
	SHA256 map[string]string
}

// AssetName is the release asset of an architecture for a version.
func (p Pin) AssetName(arch, version string) string {
	return fmt.Sprintf("actions-runner-%s-%s-%s.tar.gz", p.OS, arch, version)
}

// ParsePin reads the runner pin out of a recipe's text.
func ParsePin(text string) (Pin, error) {
	v, a := versionLine.FindStringSubmatch(text), archiveName.FindStringSubmatch(text)
	hashes := shaLine.FindAllStringSubmatch(text, -1)
	if v == nil || a == nil || len(hashes) == 0 {
		return Pin{}, errors.New("the recipe does not pin a runner (RUNNER_VERSION=, RUNNER_SHA256= and an actions-runner-<os>-<arch>-${RUNNER_VERSION}.tar.gz download)")
	}
	pin := Pin{Version: v[2], OS: a[1], SHA256: map[string]string{}}
	for _, hash := range hashes {
		arch := shaArch(hash[2], a[2])
		if arch == "" || pin.SHA256[arch] != "" {
			return Pin{}, errors.New("the recipe's RUNNER_SHA256 lines do not match its download: use RUNNER_SHA256= for a download that names its architecture, and RUNNER_SHA256_ARM64= and RUNNER_SHA256_X64= (once each) for one that uses ${arch}")
		}
		pin.SHA256[arch] = hash[3]
	}
	if a[2] == chosenArch {
		if len(pin.SHA256) != 2 {
			return Pin{}, errors.New("a recipe that picks ${arch} needs both RUNNER_SHA256_ARM64= and RUNNER_SHA256_X64=")
		}
	} else if len(pin.SHA256) != 1 {
		return Pin{}, errors.New("a recipe that downloads one architecture has exactly one RUNNER_SHA256= line")
	}
	return pin, nil
}

// shaArch is the architecture of a RUNNER_SHA256 line: its suffix, or for a bare line the one the
// download names. It is empty when the line cannot belong to the download.
func shaArch(suffix, download string) string {
	switch {
	case suffix == "ARM64" && download == chosenArch:
		return "arm64"
	case suffix == "X64" && download == chosenArch:
		return "x64"
	case suffix == "" && download != chosenArch:
		return download
	}
	return ""
}

// WithPin returns the recipe text with the version and the digests (by architecture) replaced.
func WithPin(text, version string, sha map[string]string) string {
	a := archiveName.FindStringSubmatch(text)
	text = versionLine.ReplaceAllString(text, "${1}"+version)
	return shaLine.ReplaceAllStringFunc(text, func(line string) string {
		m := shaLine.FindStringSubmatch(line)
		if a == nil {
			return line
		}
		if digest, ok := sha[shaArch(m[2], a[2])]; ok {
			return m[1] + digest
		}
		return line
	})
}

// compareVersions compares dotted numeric versions.
func compareVersions(a, b string) int {
	pa, pb := strings.Split(a, "."), strings.Split(b, ".")
	for i := 0; i < max(len(pa), len(pb)); i++ {
		var x, y int
		if i < len(pa) {
			x, _ = strconv.Atoi(pa[i])
		}
		if i < len(pb) {
			y, _ = strconv.Atoi(pb[i])
		}
		if x != y {
			if x < y {
				return -1
			}
			return 1
		}
	}
	return 0
}

// Release is a published actions/runner release.
type Release struct {
	Version   string
	Published time.Time
	// Digests maps an asset name to its SHA-256.
	Digests map[string]string
}

// ReleaseSource finds the latest actions/runner release.
type ReleaseSource interface {
	Latest(ctx context.Context) (Release, error)
}

// GitHubReleases reads the latest release from the GitHub API without credentials.
type GitHubReleases struct {
	URL  string
	HTTP *http.Client
}

// Latest implements ReleaseSource.
func (g GitHubReleases) Latest(ctx context.Context) (Release, error) {
	url := g.URL
	if url == "" {
		url = "https://api.github.com/repos/actions/runner/releases/latest"
	}
	client := g.HTTP
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Second}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return Release{}, err
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	resp, err := client.Do(req)
	if err != nil {
		return Release{}, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return Release{}, fmt.Errorf("GitHub answered %d for the latest runner release", resp.StatusCode)
	}
	var body struct {
		Tag       string    `json:"tag_name"`
		Published time.Time `json:"published_at"`
		Assets    []struct {
			Name   string `json:"name"`
			Digest string `json:"digest"`
		} `json:"assets"`
	}
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Release{}, err
	}
	if err := json.Unmarshal(raw, &body); err != nil {
		return Release{}, err
	}
	release := Release{Version: strings.TrimPrefix(body.Tag, "v"), Published: body.Published, Digests: map[string]string{}}
	for _, asset := range body.Assets {
		if sha, ok := strings.CutPrefix(asset.Digest, "sha256:"); ok {
			release.Digests[asset.Name] = sha
		}
	}
	return release, nil
}

// Refresher keeps the recipes' runner program current. GitHub requires a runner with updates
// disabled to be updated within 30 days of a new release, so a new release is applied to the
// recipe once it is AfterDays old, and the recipe is rebuilt.
type Refresher struct {
	ProjectDir string
	// Recipes maps a recipe name to the operating system of its computers. Their memory is read
	// from the recipe file.
	Recipes map[string]string
	Source  ReleaseSource
	Silo    Silo
	// Reserve says whether one more computer of the operating system and memory fits and, when it
	// does, counts it against every other decision until release is called.
	Reserve   func(ctx context.Context, os string, memoryGiB int) (release func(), err error)
	Gates     map[string]*sync.RWMutex
	AfterDays int
	Now       func() time.Time
	Logger    *slog.Logger
}

func (r *Refresher) path(recipe string) string {
	return filepath.Join(r.ProjectDir, ".silo", "recipes", recipe+".yaml")
}

// CheckOnce compares every recipe's pin with the latest release and refreshes the ones that are
// behind.
func (r *Refresher) CheckOnce(ctx context.Context) error {
	release, err := r.Source.Latest(ctx)
	if err != nil {
		return fmt.Errorf("cannot read the latest runner release: %w", err)
	}
	var failures []error
	for recipe := range r.Recipes {
		if err := r.checkRecipe(ctx, recipe, release); err != nil {
			r.Logger.Error("runner version check failed", "recipe", recipe, "error", err.Error())
			failures = append(failures, fmt.Errorf("%s: %w", recipe, err))
		}
	}
	return errors.Join(failures...)
}

func (r *Refresher) checkRecipe(ctx context.Context, recipe string, release Release) error {
	text, err := os.ReadFile(r.path(recipe))
	if err != nil {
		return fmt.Errorf("cannot read the recipe (silo-runner install-recipes puts the shipped ones in %s): %w", filepath.Dir(r.path(recipe)), err)
	}
	pin, err := ParsePin(string(text))
	if err != nil {
		return err
	}
	if compareVersions(pin.Version, release.Version) >= 0 {
		r.Logger.Info("runner version is current", "recipe", recipe, "version", pin.Version)
		return nil
	}
	age := r.Now().Sub(release.Published)
	deadline := release.Published.Add(runnerUpdateDeadlineDays * 24 * time.Hour)
	if age < time.Duration(r.AfterDays)*24*time.Hour {
		r.Logger.Info("a newer runner exists; the recipe moves to it when the release is old enough",
			"recipe", recipe, "have", pin.Version, "latest", release.Version, "ageDays", int(age.Hours()/24), "afterDays", r.AfterDays)
		return nil
	}
	// Every architecture the recipe can run on is pinned to the digest of its own asset; one
	// missing digest leaves the recipe as it is.
	sha := map[string]string{}
	for arch := range pin.SHA256 {
		digest, ok := release.Digests[pin.AssetName(arch, release.Version)]
		if !ok || len(digest) != 64 {
			return fmt.Errorf("the release %s has no digest for %s", release.Version, pin.AssetName(arch, release.Version))
		}
		sha[arch] = digest
	}
	r.Logger.Info("refreshing the runner in a recipe", "recipe", recipe, "from", pin.Version, "to", release.Version,
		"deadline", deadline.Format(time.DateOnly))
	if err := r.refresh(ctx, recipe, string(text), release.Version, sha); err != nil {
		return fmt.Errorf("%w (GitHub stops a runner that is not updated by %s)", err, deadline.Format(time.DateOnly))
	}
	return nil
}

func (r *Refresher) refresh(ctx context.Context, recipe, oldText, version string, sha map[string]string) error {
	gate := r.Gates[recipe]
	if gate == nil {
		gate = &sync.RWMutex{}
	}
	// New computers wait while the recipe is rebuilt; a rebuild that ran during a fork would race it.
	gate.Lock()
	defer gate.Unlock()
	// A rebuild runs one computer of the recipe's size, and counts as one while it runs.
	release, err := r.Reserve(ctx, r.Recipes[recipe], RecipeMemoryGiB(r.ProjectDir, recipe, r.Recipes[recipe]))
	if err != nil {
		return err
	}
	defer release()
	if err := writeAtomic(r.path(recipe), WithPin(oldText, version, sha)); err != nil {
		return err
	}
	if err := r.Silo.BuildRecipe(ctx, recipe); err != nil {
		if restoreErr := writeAtomic(r.path(recipe), oldText); restoreErr != nil {
			return errors.Join(err, restoreErr)
		}
		return fmt.Errorf("the rebuild failed and the recipe is back at its old version: %w", err)
	}
	return nil
}

func writeAtomic(path, text string) error {
	temp := path + ".tmp"
	if err := os.WriteFile(temp, []byte(text), 0o644); err != nil {
		return err
	}
	return os.Rename(temp, path)
}

// Loop checks at startup and then every interval.
func (r *Refresher) Loop(ctx context.Context, every time.Duration) {
	for {
		if err := r.CheckOnce(ctx); err != nil {
			r.Logger.Warn("runner version check", "error", err.Error())
		}
		select {
		case <-time.After(every):
		case <-ctx.Done():
			return
		}
	}
}
