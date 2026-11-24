// Go's encoding/json into the struct a Go host would use: what it sees of a body.
// usage: json_go <file>; prints {"ok":..,"web_fetch":..,"max_tokens":..}
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
)

type Req struct {
	Model     string           `json:"model"`
	MaxTokens json.Number      `json:"max_tokens"`
	Tools     []map[string]any `json:"tools"`
}

func main() {
	b, _ := os.ReadFile(os.Args[1])
	var r Req
	err := json.Unmarshal(b, &r)
	wf := false
	for _, t := range r.Tools {
		if n, _ := t["name"].(string); strings.Contains(n, "web_fetch") {
			wf = true
		}
	}
	o, _ := json.Marshal(map[string]any{"ok": err == nil, "web_fetch": wf, "max_tokens": string(r.MaxTokens)})
	fmt.Println(string(o))
}
