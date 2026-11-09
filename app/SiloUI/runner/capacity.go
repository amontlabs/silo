package main

import (
	"fmt"
	"os"
	"path/filepath"

	"gopkg.in/yaml.v3"
)

// MacOSVMLimit is Apple's limit on macOS virtual machines running at once on one Mac.
const MacOSVMLimit = 2

// Demand is a computer the helper has asked for that `silo list` does not show running yet: it
// will hold a virtual machine slot and its memory as soon as it does.
type Demand struct {
	OS        string
	MemoryGiB int
}

// Capacity decides whether the host can take another computer. It is a pure function of the
// latest `silo list --all` (so a macOS recipe build, a computer the owner started and the
// helper's own runners all count) plus the computers the helper has asked for that the list does
// not show running yet.
type Capacity struct {
	Linux LinuxBudget
}

// Allows reports whether one more computer of the operating system and memory fits.
func (c Capacity) Allows(os string, memoryGiB int, listed []Computer, pending []Demand) error {
	var macRunning, linuxRunning, linuxMem int
	for _, computer := range listed {
		if !computer.Occupies() {
			continue
		}
		// Whatever is not a macOS virtual machine runs on the Linux budget: a Linux virtual
		// machine, and a container computer, which uses the host's memory the same way.
		if computer.Kind == "macos" {
			macRunning++
		} else {
			linuxRunning++
			linuxMem += computer.MemoryGiB
		}
	}
	for _, demand := range pending {
		if demand.OS == "macos" {
			macRunning++
		} else {
			linuxRunning++
			linuxMem += demand.MemoryGiB
		}
	}
	if os == "macos" {
		if macRunning+1 > MacOSVMLimit {
			return fmt.Errorf("%w: Apple allows %d macOS virtual machines at once and %d are running or starting", errNoCapacity, MacOSVMLimit, macRunning)
		}
		return nil
	}
	if c.Linux.Count > 0 && linuxRunning+1 > c.Linux.Count {
		return fmt.Errorf("%w: %d Linux computers are running or starting, the limit is %d", errNoCapacity, linuxRunning, c.Linux.Count)
	}
	if c.Linux.MemoryGiB > 0 && linuxMem+memoryGiB > c.Linux.MemoryGiB {
		return fmt.Errorf("%w: Linux computers use %d GiB and %d GiB more are needed, the budget is %d GiB", errNoCapacity, linuxMem, memoryGiB, c.Linux.MemoryGiB)
	}
	return nil
}

// RecipeMemoryGiB reads the memory a recipe gives its computers from the recipe file in the
// project directory, or the engine's default for the operating system when the file does not say
// (or is a built-in recipe with no file).
func RecipeMemoryGiB(projectDir, recipe, osName string) int {
	fallback := 4
	if osName == "macos" {
		fallback = 8
	}
	raw, err := os.ReadFile(filepath.Join(projectDir, ".silo", "recipes", recipe+".yaml"))
	if err != nil {
		return fallback
	}
	var spec struct {
		Memory int `yaml:"memory"`
	}
	if yaml.Unmarshal(raw, &spec) != nil || spec.Memory <= 0 {
		return fallback
	}
	return spec.Memory
}
