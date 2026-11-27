// A stand-in for a Google API that speaks gRPC (Pub/Sub, Firestore, Spanner): the health service,
// one unary call (Check) and one server stream (Watch, three messages, then trailers). It accepts
// only the real token of the secret `gcp`, and logs which credential reached it, as #395's
// stand-in hosts do: one JSON line per call to LOG.
//
// usage: standin <listen ip:port> <cert chain> <key> <real token> <log>
package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"log"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type server struct {
	grpc_health_v1.UnimplementedHealthServer
	real string
	mu   sync.Mutex
	out  *os.File
}

func (s *server) seen(ctx context.Context, call string) error {
	md, _ := metadata.FromIncomingContext(ctx)
	auth := strings.Join(md.Get("authorization"), ",")
	cred := "none"
	switch {
	case auth == "Bearer "+s.real:
		cred = "real:gcp"
	case strings.Contains(auth, "placeholder"):
		cred = "placeholder"
	case auth != "":
		cred = "other"
	}
	s.mu.Lock()
	b, _ := json.Marshal(map[string]string{"host": strings.Join(md.Get(":authority"), ""), "call": call, "cred": cred})
	s.out.Write(append(b, '\n'))
	s.mu.Unlock()
	if cred != "real:gcp" {
		return status.Error(codes.Unauthenticated, "stand-in: not the real token")
	}
	return nil
}

func (s *server) Check(ctx context.Context, r *grpc_health_v1.HealthCheckRequest) (*grpc_health_v1.HealthCheckResponse, error) {
	if err := s.seen(ctx, "Check"); err != nil {
		return nil, err
	}
	return &grpc_health_v1.HealthCheckResponse{Status: grpc_health_v1.HealthCheckResponse_SERVING}, nil
}

func (s *server) Watch(r *grpc_health_v1.HealthCheckRequest, st grpc_health_v1.Health_WatchServer) error {
	if err := s.seen(st.Context(), "Watch"); err != nil {
		return err
	}
	for i := 0; i < 3; i++ {
		if err := st.Send(&grpc_health_v1.HealthCheckResponse{Status: grpc_health_v1.HealthCheckResponse_SERVING}); err != nil {
			return err
		}
		time.Sleep(20 * time.Millisecond)
	}
	st.SetTrailer(metadata.Pairs("x-standin", "watched"))
	return status.Error(codes.OK, "")
}

func main() {
	a := os.Args[1:]
	cert, err := tls.LoadX509KeyPair(a[1], a[2])
	if err != nil {
		log.Fatal(err)
	}
	out, err := os.OpenFile(a[4], os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		log.Fatal(err)
	}
	l, err := net.Listen("tcp", a[0])
	if err != nil {
		log.Fatal(err)
	}
	g := grpc.NewServer(grpc.Creds(credentials.NewTLS(&tls.Config{Certificates: []tls.Certificate{cert}})))
	grpc_health_v1.RegisterHealthServer(g, &server{real: a[3], out: out})
	log.Fatal(g.Serve(l))
}
