// Command silo-runner makes Silo the machine GitHub Actions jobs run on: every job gets a fresh
// Silo computer, thrown away after the job. It owns everything GitHub-facing (the scale set
// protocol, jobs, runner registration) and drives Silo only through its command line.
package main

import (
	"embed"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/exec"
	"runtime"
	"syscall"
)

//go:embed recipes/*.yaml
var shippedRecipes embed.FS

var version = "dev"

var terminateSignal = syscall.SIGTERM

const usage = `usage: silo-runner <command> -config FILE

  run             serve the scale sets in the configuration until interrupted
                  (-silo PATH replaces silo.path; the service passes an absolute one)
  check           validate the configuration and refuse a public repository, then exit
                  (-offline: check the configuration and this machine only, never GitHub)
  install-recipes write the silo-linux and silo-macos recipes into the project directory
                  (-replace: also replace one already there, keeping it as .bak)
  service register   [-engine-unit NAME] install and start the systemd user service (Linux); it writes
                  the absolute path of silo.path into the unit
  service unregister stop and remove the service
  version         print the version

silo-runner never runs as root: run it as the account that runs silod.
`

func main() {
	os.Exit(realMain(os.Args[1:]))
}

func realMain(args []string) int {
	if len(args) == 0 {
		fmt.Fprint(os.Stderr, usage)
		return 2
	}
	command, args := args[0], args[1:]
	if command == "version" {
		fmt.Println(version)
		return 0
	}
	if os.Geteuid() == 0 {
		fmt.Fprintln(os.Stderr, "silo-runner: refusing to run as root; run it as the account that runs silod")
		return 1
	}
	if command == "service" {
		return serviceMain(args)
	}
	flags := flag.NewFlagSet(command, flag.ContinueOnError)
	path := flags.String("config", "", "configuration file (YAML or JSON)")
	debug := flags.Bool("debug", false, "log at debug level")
	siloPath := flags.String("silo", "", "run: the silo command, replacing silo.path (the service passes an absolute path)")
	replace := flags.Bool("replace", false, "install-recipes: replace a recipe already there (the old one is kept as .bak)")
	offline := flags.Bool("offline", false, "check: look at the configuration and this machine only")
	if err := flags.Parse(args); err != nil {
		return 2
	}
	if *path == "" {
		fmt.Fprint(os.Stderr, usage)
		return 2
	}
	level := slog.LevelInfo
	if *debug {
		level = slog.LevelDebug
	}
	logger := slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: level}))
	cfg, err := LoadConfig(*path)
	if err != nil {
		fmt.Fprintln(os.Stderr, "silo-runner:", err)
		return 1
	}
	if *siloPath != "" {
		cfg.Silo.Path = *siloPath
	}
	ctx, stop := SignalContext()
	defer stop()
	switch command {
	case "install-recipes":
		err = InstallRecipes(cfg.Silo.ProjectDir, *replace, func(format string, args ...any) { fmt.Printf(format, args...) })
	case "check", "run":
		if command == "check" {
			// Before anything that can fail, so the warning is there next to the failure.
			printSecretWarnings(cfg)
		}
		if command == "check" && *offline {
			err = checkOffline(cfg)
			break
		}
		var deps *Deps
		deps, err = NewDeps(cfg, logger)
		if err != nil {
			break
		}
		if command == "check" {
			if _, err = ApplyHost(cfg, deps.Host); err == nil {
				err = CheckAccess(ctx, cfg, deps)
			}
			if err == nil {
				fmt.Println("ok: the configuration is valid and nothing public can reach these runners")
			}
		} else {
			err = Run(ctx, cfg, deps)
		}
	default:
		fmt.Fprint(os.Stderr, usage)
		return 2
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "silo-runner:", err)
		return 1
	}
	return 0
}

// printSecretWarnings says which credential files the group or others can read.
func printSecretWarnings(cfg *Config) {
	for _, warning := range SecretFileWarnings(cfg, envFile()) {
		fmt.Println("warning:", warning)
	}
}

// checkOffline validates the configuration and, on a Linux host, the machine: /dev/kvm and the
// Linux budget. It never contacts GitHub.
func checkOffline(cfg *Config) error {
	report, err := ApplyHost(cfg, RealHost())
	if err != nil {
		return err
	}
	if report.LinuxHost {
		fmt.Println(report.Describe())
	}
	fmt.Println("ok: the configuration is valid and this machine can run the computers (GitHub was not asked)")
	return nil
}

// serviceMain runs `service register|unregister`.
func serviceMain(args []string) int {
	if len(args) == 0 || (args[0] != "register" && args[0] != "unregister") {
		fmt.Fprint(os.Stderr, usage)
		return 2
	}
	action := args[0]
	if runtime.GOOS != "linux" {
		fmt.Fprintln(os.Stderr, "silo-runner: the service is a systemd user unit, for Linux hosts; on a Mac run silo-runner from a LaunchAgent or a terminal")
		return 1
	}
	flags := flag.NewFlagSet("service "+action, flag.ContinueOnError)
	path := flags.String("config", "", "configuration file (YAML or JSON)")
	engineUnit := flags.String("engine-unit", defaultEngineUnit, "the engine's systemd user unit to start after")
	if err := flags.Parse(args[1:]); err != nil {
		return 2
	}
	setup, err := DefaultServiceSetup(os.Stdout)
	if err == nil && action == "unregister" {
		err = UnregisterService(setup)
	} else if err == nil {
		if *path == "" {
			fmt.Fprint(os.Stderr, usage)
			return 2
		}
		// A configuration that does not load would only fail again under systemd, and so would a
		// silo the user manager's PATH cannot find: resolve it here, from this shell.
		var cfg *Config
		if cfg, err = LoadConfig(*path); err == nil {
			var program, silo string
			if silo, err = ResolveSilo(cfg.Silo.Path, exec.LookPath); err == nil {
				if program, err = os.Executable(); err == nil {
					err = RegisterService(setup, program, *path, silo, *engineUnit)
				}
			}
		}
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "silo-runner:", err)
		return 1
	}
	return 0
}
