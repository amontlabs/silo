// Go's x/crypto/ssh logs in through the boundary with the agent's key only.
package main

import (
	"fmt"
	"net"
	"os"

	"golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/agent"
)

func main() {
	a, err := net.Dial("unix", os.Args[1])
	if err != nil { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
	cfg := &ssh.ClientConfig{User: "deploy", HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Auth: []ssh.AuthMethod{ssh.PublicKeysCallback(agent.NewClient(a).Signers)}}
	c, err := ssh.Dial("tcp", os.Args[2], cfg)
	if err != nil { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
	s, _ := c.NewSession()
	out, _ := s.Output("echo ok")
	fmt.Print(string(out))
}
