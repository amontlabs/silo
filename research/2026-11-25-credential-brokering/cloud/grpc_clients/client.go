// A Go gRPC client sending authorization: Bearer <token> three times to host:port.
package main

import (
	"context"
	"fmt"
	"os"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

type raw struct{}

func (raw) Marshal(v any) ([]byte, error)      { return *(v.(*[]byte)), nil }
func (raw) Unmarshal(d []byte, v any) error    { *(v.(*[]byte)) = d; return nil }
func (raw) Name() string                        { return "proto" }

func main() {
	c, err := grpc.NewClient(os.Args[1], grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.ForceCodec(raw{})))
	if err != nil { fmt.Println(err); os.Exit(1) }
	for i := 0; i < 3; i++ {
		ctx := metadata.AppendToOutgoingContext(context.Background(), "authorization", "Bearer "+os.Args[2])
		in, out := []byte("ping"), []byte{}
		if err := c.Invoke(ctx, "/demo.Echo/Ping", &in, &out); err != nil { fmt.Println("err", err) }
	}
	fmt.Println("done")
}
