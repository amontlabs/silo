// A gRPC client in a computer, as Google's Go libraries call Pub/Sub: TLS to HOST:443 with the
// system's authorities (SSL_CERT_FILE), the computer's placeholder as a bearer token, one unary
// call and one server stream. gRPC-Go takes HTTPS_PROXY, so on c1 it goes by #378's CONNECT.
// Prints one line per call and exits 1 if any failed.
//
// usage: client <host> <token>
package main

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"io"
	"os"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/metadata"
)

func main() {
	host, tok := os.Args[1], os.Args[2]
	pool, _ := x509.SystemCertPool()
	cc, err := grpc.NewClient(host+":443", grpc.WithTransportCredentials(credentials.NewTLS(&tls.Config{RootCAs: pool, ServerName: host})))
	if err != nil {
		fmt.Println("dial:", err)
		os.Exit(1)
	}
	defer cc.Close()
	h := grpc_health_v1.NewHealthClient(cc)
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	ctx = metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+tok)
	failed := false
	r, err := h.Check(ctx, &grpc_health_v1.HealthCheckRequest{})
	if err != nil {
		fmt.Println("Check: error:", err)
		failed = true
	} else {
		fmt.Println("Check:", r.Status)
	}
	var tr metadata.MD
	st, err := h.Watch(ctx, &grpc_health_v1.HealthCheckRequest{}, grpc.Trailer(&tr))
	n := 0
	for err == nil {
		_, err = st.Recv()
		if err == nil {
			n++
		}
	}
	if err != io.EOF {
		fmt.Println("Watch: error:", err)
		failed = true
	} else {
		fmt.Printf("Watch: %d messages, trailer x-standin=%v\n", n, tr.Get("x-standin"))
	}
	if failed {
		os.Exit(1)
	}
}
