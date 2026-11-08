package main

import (
	"fmt"
	"os"
	"path/filepath"
)

// envFile is the optional file the service unit reads (EnvironmentFile=-%E/silo-runner/env): the
// place for SILO_RUNNER_TOKEN= when github.auth.tokenEnv is used.
func envFile() string {
	dir, err := userConfigDir()
	if err != nil {
		return ""
	}
	return filepath.Join(dir, "silo-runner", "env")
}

// SecretFileWarnings names the files that hold the GitHub credential and that the group or others
// can read: the GitHub App private key and the service's environment file. Like ssh with a
// private key, it only warns; a missing file is not its business (the helper says so when it
// needs the file). Each warning is one sentence that ends with the fix.
func SecretFileWarnings(cfg *Config, env string) []string {
	var files []string
	if app := cfg.GitHub.Auth.App; app != nil {
		files = append(files, app.PrivateKeyFile)
	}
	if env != "" {
		files = append(files, env)
	}
	var warnings []string
	for _, path := range files {
		info, err := os.Stat(path)
		if err != nil || !info.Mode().IsRegular() {
			continue
		}
		if mode := info.Mode().Perm(); mode&0o077 != 0 {
			warnings = append(warnings, fmt.Sprintf("%s holds a GitHub credential and its mode is %04o, so other accounts on this machine can read it: chmod 600 %s", path, mode, path))
		}
	}
	return warnings
}
