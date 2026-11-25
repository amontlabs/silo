// One GET through the proxy in the environment, as a Go program (the gcloud and Terraform kind)
// would make it. Go reads SSL_CERT_FILE for its store.
package main

import (
	"fmt"
	"net/http"
	"os"
	"time"
)

func main() {
	c := &http.Client{Timeout: 10 * time.Second}
	r, err := c.Get(os.Args[1])
	if err != nil {
		fmt.Println(err)
		os.Exit(1)
	}
	fmt.Println(r.Status)
}
