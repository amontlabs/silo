// Prints the headers Go's net/http (as Ollama uses) reads from each request.
package main

import (
	"encoding/json"
	"net/http"
	"os"
)

func main() {
	http.ListenAndServe("127.0.0.1:"+os.Args[1], http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, _ := json.Marshal(map[string]any{"headers": r.Header, "host": r.Host})
		w.Header().Set("content-type", "application/json")
		w.Write(b)
	}))
}
