package main

import (
	"context"
	"crypto/rsa"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

// publicRepoAdvice is shown whenever the helper refuses a public repository.
const publicRepoAdvice = "GitHub's own guidance is that self-hosted runners \"should almost never be used for public repositories\": " +
	"anyone can open a pull request, and a workflow it runs would execute on this Mac. silo-runner only serves private repositories"

// AccessChecker verifies that nothing public can reach the runners.
type AccessChecker interface {
	CheckPrivate(ctx context.Context, target Target, runnerGroup string) error
}

// RESTChecker asks GitHub's REST API.
type RESTChecker struct {
	APIBase string
	Token   func(ctx context.Context) (string, error)
	HTTP    *http.Client
}

// APIBaseFor returns the REST base of a GitHub host.
func APIBaseFor(host string) string {
	if host == "github.com" {
		return "https://api.github.com"
	}
	return "https://" + host + "/api/v3"
}

func (c *RESTChecker) get(ctx context.Context, path string, into any) (int, error) {
	token, err := c.Token(ctx)
	if err != nil {
		return 0, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.APIBase+path, nil)
	if err != nil {
		return 0, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/vnd.github+json")
	req.Header.Set("X-GitHub-Api-Version", "2022-11-28")
	client := c.HTTP
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Second}
	}
	resp, err := client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if resp.StatusCode != http.StatusOK {
		return resp.StatusCode, nil
	}
	return resp.StatusCode, json.Unmarshal(body, into)
}

// CheckPrivate refuses a public repository, and for an organization a runner group that allows
// public repositories. It also refuses when it cannot tell: an unverified setup is not safe.
func (c *RESTChecker) CheckPrivate(ctx context.Context, target Target, runnerGroup string) error {
	if !target.IsOrg() {
		var repo struct {
			Private    bool   `json:"private"`
			Visibility string `json:"visibility"`
		}
		status, err := c.get(ctx, fmt.Sprintf("/repos/%s/%s", url.PathEscape(target.Owner), url.PathEscape(target.Repo)), &repo)
		switch {
		case err != nil:
			return fmt.Errorf("cannot check whether %s/%s is private: %w", target.Owner, target.Repo, err)
		case status != http.StatusOK:
			return fmt.Errorf("cannot check whether %s/%s is private: GitHub answered %d (the credential needs read access to the repository's metadata)", target.Owner, target.Repo, status)
		case !repo.Private || repo.Visibility == "public":
			return fmt.Errorf("%s/%s is a public repository. %s", target.Owner, target.Repo, publicRepoAdvice)
		}
		return nil
	}
	var groups struct {
		RunnerGroups []struct {
			Name              string `json:"name"`
			AllowsPublicRepos *bool  `json:"allows_public_repositories"`
			Visibility        string `json:"visibility"`
		} `json:"runner_groups"`
	}
	status, err := c.get(ctx, fmt.Sprintf("/orgs/%s/actions/runner-groups?per_page=100", url.PathEscape(target.Owner)), &groups)
	switch {
	case err != nil:
		return fmt.Errorf("cannot read the runner groups of %s: %w", target.Owner, err)
	case status != http.StatusOK:
		return fmt.Errorf("cannot read the runner groups of %s: GitHub answered %d (the credential needs the organization permission to read self-hosted runners)", target.Owner, status)
	}
	for _, group := range groups.RunnerGroups {
		if !strings.EqualFold(group.Name, runnerGroup) {
			continue
		}
		if group.AllowsPublicRepos == nil {
			return fmt.Errorf("the runner group %q of %s does not report allows_public_repositories, so it cannot be shown to exclude public repositories", runnerGroup, target.Owner)
		}
		if *group.AllowsPublicRepos {
			return fmt.Errorf("the runner group %q of %s allows public repositories. %s. Turn off \"Allow public repositories\" for the group, or use another group", runnerGroup, target.Owner, publicRepoAdvice)
		}
		return nil
	}
	return fmt.Errorf("the organization %s has no runner group called %q", target.Owner, runnerGroup)
}

// TokenSource gives the REST checker a bearer token: the personal access token, or an
// installation token minted from the GitHub App's key.
func TokenSource(auth AuthConfig, apiBase string, httpClient *http.Client, now func() time.Time) (func(context.Context) (string, error), error) {
	if auth.TokenEnv != "" {
		return func(context.Context) (string, error) {
			token := os.Getenv(auth.TokenEnv)
			if token == "" {
				return "", fmt.Errorf("the environment variable %s is empty", auth.TokenEnv)
			}
			return token, nil
		}, nil
	}
	pem, err := os.ReadFile(auth.App.PrivateKeyFile)
	if err != nil {
		return nil, fmt.Errorf("cannot read the GitHub App private key: %w", err)
	}
	key, err := jwt.ParseRSAPrivateKeyFromPEM(pem)
	if err != nil {
		return nil, fmt.Errorf("the GitHub App private key is not a PEM RSA key: %w", err)
	}
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	var (
		mu      sync.Mutex
		cached  string
		expires time.Time
	)
	return func(ctx context.Context) (string, error) {
		mu.Lock()
		defer mu.Unlock()
		if cached != "" && now().Before(expires.Add(-time.Minute)) {
			return cached, nil
		}
		token, exp, err := installationToken(ctx, httpClient, apiBase, auth.App, key, now())
		if err != nil {
			return "", err
		}
		cached, expires = token, exp
		return cached, nil
	}, nil
}

func installationToken(ctx context.Context, client *http.Client, apiBase string, app *AppAuth, key *rsa.PrivateKey, now time.Time) (string, time.Time, error) {
	signed, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.RegisteredClaims{
		Issuer:    app.ClientID,
		IssuedAt:  jwt.NewNumericDate(now.Add(-30 * time.Second)),
		ExpiresAt: jwt.NewNumericDate(now.Add(9 * time.Minute)),
	}).SignedString(key)
	if err != nil {
		return "", time.Time{}, err
	}
	endpoint := fmt.Sprintf("%s/app/installations/%d/access_tokens", apiBase, app.InstallationID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, nil)
	if err != nil {
		return "", time.Time{}, err
	}
	req.Header.Set("Authorization", "Bearer "+signed)
	req.Header.Set("Accept", "application/vnd.github+json")
	resp, err := client.Do(req)
	if err != nil {
		return "", time.Time{}, err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if resp.StatusCode != http.StatusCreated {
		return "", time.Time{}, fmt.Errorf("GitHub answered %d to the installation token request", resp.StatusCode)
	}
	var reply struct {
		Token     string    `json:"token"`
		ExpiresAt time.Time `json:"expires_at"`
	}
	if err := json.Unmarshal(body, &reply); err != nil || reply.Token == "" {
		return "", time.Time{}, errors.New("the installation token reply was unreadable")
	}
	return reply.Token, reply.ExpiresAt, nil
}
