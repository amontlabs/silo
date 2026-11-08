package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// The helper forgets a forking record, deleting nothing, only when the fork's error carries
// this exact engine sentence; a reworded message would make it wait for a list and bind the
// colliding computer instead.
func TestTheNameTakenMessageIsTheEnginesOwn(t *testing.T) {
	engine := filepath.Join("..", "src-tauri", "engine", "src")
	for _, source := range []struct{ file, text string }{
		{filepath.Join(engine, "computer_names.rs"), `const TAKEN: &str = "` + nameTakenMessage + `";`},
		{filepath.Join(engine, "runtime", "mod.rs"), `"` + nameTakenMessage + `".into()`},
	} {
		raw, err := os.ReadFile(source.file)
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(string(raw), source.text) {
			t.Errorf("%s no longer contains %s", source.file, source.text)
		}
	}
}
