// A stand-in provider host for the boundary's end-to-end tests. It reads requests with Go's
// net/http, routes the way the real hosts were seen to route (a cleaned path, absolute-form
// accepted), logs every request it served, and answers each model call with a fixed usage.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path"
	"strings"
)

func main() {
	logf, _ := os.OpenFile(os.Args[1], os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		clean := path.Clean("/" + r.URL.Path)
		rec, _ := json.Marshal(map[string]any{"method": r.Method, "target": r.RequestURI, "clean": clean, "body_len": len(body), "cl": r.ContentLength, "te": r.TransferEncoding})
		fmt.Fprintln(logf, string(rec))
		switch {
		case r.Method == "POST" && clean == "/v1/messages":
			w.Header().Set("content-type", "application/json")
			io.WriteString(w, `{"type":"message","id":"msg_1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":1000000,"output_tokens":1000000}}`)
		case r.Method == "POST" && clean == "/v1/responses":
			w.Header().Set("content-type", "application/json")
			io.WriteString(w, `{"object":"response","id":"resp_1","status":"completed","model":"gpt-6.2-sol","output":[],"usage":{"input_tokens":1000000,"output_tokens":1000000}}`)
		case r.Method == "POST" && strings.HasSuffix(strings.TrimSuffix(clean, "/"), ":generateContent"):
			w.Header().Set("content-type", "application/json")
			io.WriteString(w, `{"candidates":[],"modelVersion":"gemini-3.1-pro-preview","usageMetadata":{"promptTokenCount":1000000,"candidatesTokenCount":1000000}}`)
		default:
			w.WriteHeader(404)
			io.WriteString(w, `{"error":"not found"}`)
		}
	})
	// No ServeMux: a mux would redirect unclean paths; the real hosts answered them (see README).
	srv := &http.Server{Addr: os.Args[2], Handler: h}
	panic(srv.ListenAndServeTLS(os.Args[3], os.Args[4]))
}
