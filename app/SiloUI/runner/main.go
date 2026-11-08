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
	"syscall"
)

//go:embed recipes/*.yaml
var shippedRecipes embed.FS

var version = "dev"

var terminateSignal = syscall.SIGTERM

const usage = `usage: silo-runner <command> -config FILE

  run             serve the scale sets in the configuration until interrupted
  check           validate the configuration and refuse a public repository, then exit
  install-recipes write the silo-linux and silo-macos recipes into the project directory
  version         print the version
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
	flags := flag.NewFlagSet(command, flag.ContinueOnError)
	path := flags.String("config", "", "configuration file (YAML or JSON)")
	debug := flags.Bool("debug", false, "log at debug level")
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
	ctx, stop := SignalContext()
	defer stop()
	switch command {
	case "install-recipes":
		err = InstallRecipes(cfg.Silo.ProjectDir, func(format string, args ...any) { fmt.Printf(format, args...) })
	case "check", "run":
		var deps *Deps
		deps, err = NewDeps(cfg, logger)
		if err != nil {
			break
		}
		if command == "check" {
			err = CheckAccess(ctx, cfg, deps)
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
