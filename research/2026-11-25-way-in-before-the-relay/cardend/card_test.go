package main

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"gvisor.dev/gvisor/pkg/buffer"
	"gvisor.dev/gvisor/pkg/tcpip"
	"gvisor.dev/gvisor/pkg/tcpip/adapters/gonet"
	"gvisor.dev/gvisor/pkg/tcpip/header"
	"gvisor.dev/gvisor/pkg/tcpip/link/channel"
	"gvisor.dev/gvisor/pkg/tcpip/link/ethernet"
	"gvisor.dev/gvisor/pkg/tcpip/network/arp"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
	"gvisor.dev/gvisor/pkg/tcpip/transport/tcp"
)

// rig is a card in way-in mode that has learned main at 169.254.77.7, with every frame it sends
// to main kept (and, with a main stack attached, delivered to it).
type rig struct {
	k    *card
	mu   sync.Mutex
	out  [][]byte
	main *channel.Endpoint // main's card, when main is a real stack
	feed chan []byte       // when set, every frame from main goes through one reader, as in main()
}

func newRig(t testing.TB) *rig {
	r := &rig{}
	r.k = &card{
		c:      &counter{kinds: map[string]int{}, ports: map[string][]string{}, path: filepath.Join(t.TempDir(), "counts.json")},
		router: net.ParseIP("10.254.0.1"),
		guest:  make(chan tcpip.Address, 1),
	}
	r.k.send = func(b []byte) {
		r.mu.Lock()
		r.out = append(r.out, append([]byte(nil), b...))
		m := r.main
		r.mu.Unlock()
		if m != nil {
			pkt := stack.NewPacketBuffer(stack.PacketBufferOptions{Payload: buffer.MakeWithData(append([]byte(nil), b...))})
			m.InjectInbound(0, pkt)
			pkt.DecRef()
		}
	}
	r.k.s, r.k.ch = newStack()
	go r.k.pump()
	r.k.frame(announce())
	select {
	case <-r.k.guest:
	case <-time.After(time.Second):
		t.Fatal("the card did not learn main's address")
	}
	t.Cleanup(func() { r.k.s.Close(); r.k.s.Wait() })
	return r
}

func (r *rig) sent() [][]byte {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([][]byte(nil), r.out...)
}

func (r *rig) toStack() int {
	r.k.c.mu.Lock()
	defer r.k.c.mu.Unlock()
	return r.k.c.kinds["for the way in"]
}

// mainStack makes main a real TCP/IP stack at 169.254.77.7 with sshd's port 22 echoing back.
func (r *rig) mainStack(t testing.TB) {
	ch := channel.New(256, 1500, tcpip.LinkAddress(mainMAC))
	s := stack.New(stack.Options{
		NetworkProtocols:   []stack.NetworkProtocolFactory{ipv4.NewProtocol, arp.NewProtocol},
		TransportProtocols: []stack.TransportProtocolFactory{tcp.NewProtocol},
	})
	s.CreateNIC(1, ethernet.New(ch))
	s.AddProtocolAddress(1, tcpip.ProtocolAddress{Protocol: ipv4.ProtocolNumber,
		AddressWithPrefix: tcpip.AddressWithPrefix{Address: tcpip.AddrFrom4Slice(mainLL), PrefixLen: 16}}, stack.AddressProperties{})
	sub, _ := tcpip.NewSubnet(tcpip.AddrFrom4([4]byte{169, 254}), tcpip.MaskFromBytes(net.CIDRMask(16, 32)))
	s.SetRouteTable([]tcpip.Route{{Destination: sub, NIC: 1}})
	l, err := gonet.ListenTCP(s, tcpip.FullAddress{NIC: 1, Port: 22}, ipv4.ProtocolNumber)
	if err != nil {
		t.Fatal(err)
	}
	go func() {
		for {
			c, err := l.Accept()
			if err != nil {
				return
			}
			go func() { io.Copy(c, c); c.Close() }()
		}
	}()
	go func() {
		for {
			pkt := ch.ReadContext(t.Context())
			if pkt == nil {
				return
			}
			v := pkt.ToView()
			if r.feed != nil {
				r.feed <- append([]byte(nil), v.AsSlice()...)
			} else {
				r.k.frame(append([]byte(nil), v.AsSlice()...))
			}
			v.Release()
			pkt.DecRef()
		}
	}()
	r.mu.Lock()
	r.main = ch
	r.mu.Unlock()
	t.Cleanup(func() { l.Close(); s.Close(); s.Wait() })
}

// The way in still works: one connection to main's port 22 carries 2 MB both ways intact.
func TestWayInEndToEnd(t *testing.T) {
	r := newRig(t)
	r.mainStack(t)
	c, err := r.k.dial(tcpip.AddrFrom4Slice(mainLL))
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	blob := make([]byte, 2<<20)
	for i := range blob {
		blob[i] = byte(i * 7)
	}
	go func() { c.Write(blob); c.(*portConn).CloseWrite() }()
	got, err := io.ReadAll(c)
	if err != nil || !bytes.Equal(got, blob) {
		t.Fatalf("copy broken: %d bytes back, err %v", len(got), err)
	}
}

// A main sending to a new address with every frame must not grow the count without end: it is
// the card's memory and the file the record reads every half second.
func TestCountsStayBounded(t *testing.T) {
	r := newRig(t)
	for i := 0; i < 50000; i++ {
		d := rnd4()
		r.k.frame(ethf(bcast, 0x0806, arpf(1, mainMAC, mainLL, make([]byte, 6), d)))
		r.k.frame(ethf(bcast, 0x0800, ip4(mainLL, d, 6, 0, nil, tcpf(mainLL, d, 40000, 443, fSYN, 1, 0, 65535, nil, nil))))
		r.k.frame(ethf(bcast, 0x0800, ip4(mainLL, d, 1, 0, nil, []byte{8, 0, 0, 0, 0, 0, 0, 0})))
	}
	r.k.c.write()
	b, _ := os.ReadFile(r.k.c.path)
	var lines [][2]any
	json.Unmarshal(b, &lines)
	if len(lines) > 300 || len(b) > 64<<10 {
		t.Fatalf("150,000 frames to new addresses left %d lines, %d bytes in the count", len(lines), len(b))
	}
	// Nothing is lost from the totals.
	total := 0
	for _, l := range lines {
		total += int(l[1].(float64))
	}
	if total < 150000 {
		t.Fatalf("the count holds %d frames of 150,000", total)
	}
}

// Only the frames of the one connection reach the stack. Each of these is something main can
// send that the way in does not need.
func TestStackSeesOnlyItsConnection(t *testing.T) {
	g := mainLL
	cases := []struct {
		name string
		f    []byte
	}{
		{"SYN from port 22 to a port with no connection", ethf(bcast, 0x0800, ip4(g, cardLL, 6, 0, nil, tcpf(g, cardLL, 22, 1234, fSYN, 1, 0, 65535, nil, nil)))},
		{"ACK from port 22 of another 169.254 address", ethf(bcast, 0x0800, ip4([]byte{169, 254, 9, 9}, cardLL, 6, 0, nil, tcpf([]byte{169, 254, 9, 9}, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, nil)))},
		{"first fragment from port 22", ethf(bcast, 0x0800, ip4(g, cardLL, 6, 0x2000, nil, tcpf(g, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, make([]byte, 64))))},
		{"later fragment whose data reads as port 22", ethf(bcast, 0x0800, ip4(g, cardLL, 6, 0x0001, nil, append([]byte{0, 22}, make([]byte, 62)...)))},
		{"IPv4 options", ethf(bcast, 0x0800, ip4(g, cardLL, 6, 0, []byte{0x07, 0x03, 0x04, 0x00}, tcpf(g, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, nil)))},
		{"a 64 KB frame", ethf(bcast, 0x0800, ip4(g, cardLL, 6, 0, nil, tcpf(g, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, make([]byte, 60000))))},
		{"ARP for the card from another address", ethf(bcast, 0x0806, arpf(1, mainMAC, []byte{169, 254, 9, 9}, make([]byte, 6), cardLL))},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			r := newRig(t)
			before := r.toStack()
			r.k.frame(c.f)
			if r.toStack() != before {
				t.Errorf("reached the stack")
			}
		})
	}
}

// A frame the card drops makes it send nothing: main cannot make the card talk, to main or to
// any address, with frames the way in did not ask for.
func TestNothingSentForStrayFrames(t *testing.T) {
	r := newRig(t)
	for i := 0; i < 1000; i++ {
		s := []byte{169, 254, byte(i >> 8), byte(i)}
		r.k.frame(ethf(bcast, 0x0800, ip4(s, cardLL, 6, 0, nil, tcpf(s, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, nil))))
		r.k.frame(ethf(bcast, 0x0800, ip4(mainLL, cardLL, 6, 0, nil, tcpf(mainLL, cardLL, 22, uint16(1000+i), fSYN, 1, 0, 65535, nil, nil))))
	}
	time.Sleep(200 * time.Millisecond)
	if n := len(r.sent()); n != 0 {
		f := r.sent()[0]
		t.Fatalf("2,000 stray frames made the card send %d frames, the first %s", n, describe(f))
	}
}

func describe(f []byte) string {
	if len(f) < 14 {
		return "runt"
	}
	switch binary.BigEndian.Uint16(f[12:]) {
	case 0x0806:
		a := header.ARP(f[14:])
		return fmt.Sprintf("ARP op %d for %v", a.Op(), net.IP(a.ProtocolAddressTarget()))
	case 0x0800:
		ip := header.IPv4(f[14:])
		return fmt.Sprintf("IPv4 to %v proto %d", ip.DestinationAddress(), ip.Protocol())
	}
	return fmt.Sprintf("ethertype %x", f[12:14])
}
