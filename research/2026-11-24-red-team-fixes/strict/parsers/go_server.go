// Go's net/http server, default settings: what it parsed, as the reply's body.
package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
)

func main() {
	http.ListenAndServe("127.0.0.1:9102", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, err := io.ReadAll(r.Body)
		if err != nil {
			w.WriteHeader(400)
			return
		}
		s := sha256.Sum256(b)
		o, _ := json.Marshal(map[string]any{"parsed": map[string]any{"method": r.Method, "target": r.RequestURI, "host": r.Host, "len": len(b), "sha": hex.EncodeToString(s[:])[:12]}})
		w.Header().Set("content-type", "application/json")
		w.Write(o)
	}))
}
