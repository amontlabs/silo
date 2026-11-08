package main

import (
	"fmt"
	"syscall"
)

// FreeDiskGiB returns the space available to a normal user under path.
func FreeDiskGiB(path string) (int, error) {
	var stat syscall.Statfs_t
	if err := syscall.Statfs(path, &stat); err != nil {
		return 0, err
	}
	return int(uint64(stat.Bavail) * uint64(stat.Bsize) >> 30), nil
}

// DiskGuard refuses new computers when the disk is nearly full.
type DiskGuard struct {
	Path   string
	MinGiB int
	Free   func(path string) (int, error)
}

// Check returns an errNoCapacity error when less than MinGiB is free.
func (d DiskGuard) Check() error {
	free := d.Free
	if free == nil {
		free = FreeDiskGiB
	}
	got, err := free(d.Path)
	if err != nil {
		return fmt.Errorf("cannot read the free disk space of %s: %w", d.Path, err)
	}
	if got < d.MinGiB {
		return fmt.Errorf("%w: %d GiB free under %s, at least %d GiB are needed", errNoCapacity, got, d.Path, d.MinGiB)
	}
	return nil
}
