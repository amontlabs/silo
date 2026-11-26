// cardend: the Mac's end of a computer's network card, with no network stack behind it.
//
// It reads the computer's Ethernet frames from a unix datagram socket, one frame per datagram,
// as Apple's VZFileHandleNetworkDeviceAttachment hands them over. It forwards nothing anywhere.
//
//	cardend -frames <unixgram socket> -counts <file> [-dial <unix socket>] [-arp-router 10.254.0.1]
//
// Without -dial it is #378's card that leads nowhere: it answers ARP for one router address,
// drops every other frame, and counts what it dropped by kind (the trace #382's gap 10 asks for).
//
// With -dial it is also the way in before the relay: each connection accepted on that unix socket
// becomes one TCP connection from the Mac's end to port 22 of the computer, at the address the
// computer gave itself when no DHCP server answered (169.254.x.y, learned from the computer's own
// ARP frames), so no address is handed out and no route exists. The stack that makes this
// connection is gVisor's netstack, and it only ever sees the frames of that connection: ARP about
// the Mac end's own address, and TCP from the computer's port 22 to the Mac end. Every other frame is dropped and counted before the stack sees
// it, so nothing the computer sends can open a connection, reach a listener or be routed.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"os/signal"
	"slices"
	"sort"
	"strings"
	"sync"
	"syscall"
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

var (
	ourMAC = net.HardwareAddr{0x5a, 0x94, 0xef, 0x00, 0x00, 0x01}
	ourLL  = tcpip.AddrFrom4([4]byte{169, 254, 81, 16}) // the Mac end's own link-local address
)

type counter struct {
	mu    sync.Mutex
	kinds map[string]int
	ports map[string][]string // the first few ports seen for a kind
	path  string
}

// maxKinds bounds the count. Main chooses every address it sends to, so without a bound one line
// per address grows the card's memory, and the file written every half second, without end.
const maxKinds = 256

// add counts one frame. A kind ending in ":<port>" is counted by its destination address, with
// its first five ports kept, so a scan is one line and not 65,535. Once maxKinds lines exist, a
// new address is counted on its kind's "other addresses" line ("tcp connect, other addresses").
func (c *counter) add(k string) {
	port := ""
	if i := strings.LastIndex(k, ":"); i > 0 && strings.HasPrefix(k, "tcp connect") || strings.HasPrefix(k, "udp") && i > 0 {
		k, port = k[:i], k[i+1:]
	}
	c.mu.Lock()
	if _, ok := c.kinds[k]; !ok && len(c.kinds) >= maxKinds {
		if i := strings.LastIndexByte(k, ' '); i > 0 {
			k = k[:i]
		}
		k += ", other addresses"
	}
	c.kinds[k]++
	if port != "" && len(c.ports[k]) < 5 && !slices.Contains(c.ports[k], port) {
		c.ports[k] = append(c.ports[k], port)
	}
	c.mu.Unlock()
}

func (c *counter) write() {
	c.mu.Lock()
	defer c.mu.Unlock()
	keys := make([]string, 0, len(c.kinds))
	for k := range c.kinds {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	out := make([][2]any, 0, len(keys))
	for _, k := range keys {
		name := k
		if p := c.ports[k]; len(p) > 0 {
			name += " ports " + strings.Join(p, ",")
			if c.kinds[k] > len(p) {
				name += ",..."
			}
		}
		out = append(out, [2]any{name, c.kinds[k]})
	}
	b, _ := json.Marshal(out)
	os.WriteFile(c.path+".tmp", append(b, '\n'), 0o644)
	os.Rename(c.path+".tmp", c.path)
}

// classify names a frame the way a person reads it in the record: what it tried to reach.
func classify(f []byte) string {
	if len(f) < header.EthernetMinimumSize {
		return "runt"
	}
	eth := header.Ethernet(f)
	p := f[header.EthernetMinimumSize:]
	switch eth.Type() {
	case header.ARPProtocolNumber:
		if len(p) >= header.ARPSize {
			a := header.ARP(p)
			return fmt.Sprintf("arp who-has %v", net.IP(a.ProtocolAddressTarget()))
		}
		return "arp"
	case header.IPv4ProtocolNumber:
		if len(p) < header.IPv4MinimumSize {
			return "ipv4 short"
		}
		ip := header.IPv4(p)
		if !ip.IsValid(len(p)) {
			return "ipv4 malformed"
		}
		dst := ip.DestinationAddress()
		switch ip.TransportProtocol() {
		case header.TCPProtocolNumber:
			t := header.TCP(ip.Payload())
			if len(t) >= header.TCPMinimumSize && t.Flags()&header.TCPFlagSyn != 0 {
				return fmt.Sprintf("tcp connect %v:%d", dst, t.DestinationPort())
			}
			return "tcp other"
		case header.UDPProtocolNumber:
			u := header.UDP(ip.Payload())
			if len(u) >= header.UDPMinimumSize {
				return fmt.Sprintf("udp %v:%d", dst, u.DestinationPort())
			}
		case header.ICMPv4ProtocolNumber:
			return fmt.Sprintf("icmp %v", dst)
		}
		return fmt.Sprintf("ipv4 proto %d", ip.Protocol())
	case header.IPv6ProtocolNumber:
		if len(p) < header.IPv6MinimumSize {
			return "ipv6 short"
		}
		ip := header.IPv6(p)
		dst := ip.DestinationAddress()
		pl := p[header.IPv6MinimumSize:]
		switch ip.TransportProtocol() {
		case header.TCPProtocolNumber:
			t := header.TCP(pl)
			if len(t) >= header.TCPMinimumSize && t.Flags()&header.TCPFlagSyn != 0 {
				return fmt.Sprintf("tcp connect [%v]:%d", dst, t.DestinationPort())
			}
			return "tcp other"
		case header.UDPProtocolNumber:
			if len(pl) >= header.UDPMinimumSize {
				return fmt.Sprintf("udp [%v]:%d", dst, header.UDP(pl).DestinationPort())
			}
		case header.ICMPv6ProtocolNumber:
			if len(pl) >= 1 {
				return fmt.Sprintf("icmpv6 type %d", pl[0])
			}
		}
		return fmt.Sprintf("ipv6 next %d", ip.NextHeader())
	}
	return fmt.Sprintf("ethertype 0x%04x", uint16(eth.Type()))
}

// arpReply answers "who has <router>" with a made-up MAC, as #378's dead-end card does.
func arpReply(f []byte, router net.IP) []byte {
	if len(f) < header.EthernetMinimumSize+header.ARPSize {
		return nil
	}
	eth := header.Ethernet(f)
	if eth.Type() != header.ARPProtocolNumber {
		return nil
	}
	a := header.ARP(f[header.EthernetMinimumSize:])
	if !a.IsValid() || a.Op() != header.ARPRequest || !net.IP(a.ProtocolAddressTarget()).Equal(router.To4()) {
		return nil
	}
	out := make([]byte, header.EthernetMinimumSize+header.ARPSize)
	header.Ethernet(out).Encode(&header.EthernetFields{
		SrcAddr: tcpip.LinkAddress(ourMAC), DstAddr: eth.SourceAddress(), Type: header.ARPProtocolNumber,
	})
	r := header.ARP(out[header.EthernetMinimumSize:])
	r.SetIPv4OverEthernet()
	r.SetOp(header.ARPReply)
	copy(r.HardwareAddressSender(), ourMAC)
	copy(r.ProtocolAddressSender(), router.To4())
	copy(r.HardwareAddressTarget(), a.HardwareAddressSender())
	copy(r.ProtocolAddressTarget(), a.ProtocolAddressSender())
	return out
}

// maxFrame is the largest frame of a 1,500-byte MTU. The stack is never handed more.
const maxFrame = header.EthernetMinimumSize + 1500

// forStack is true only for the frames the open connections need: ARP between main's learned
// address and the Mac end's, and TCP from main's port 22 to a port the Mac end is connected from,
// unfragmented, with no IP options. Nothing else reaches the stack, so main can neither open a
// connection nor make the stack answer, ask or route anything.
func (k *card) forStack(f []byte) bool {
	if len(f) < header.EthernetMinimumSize || len(f) > maxFrame {
		return false
	}
	k.mu.Lock()
	g, ok := k.guestAddr, k.learned
	k.mu.Unlock()
	if !ok {
		return false
	}
	p := f[header.EthernetMinimumSize:]
	switch header.Ethernet(f).Type() {
	case header.ARPProtocolNumber:
		if len(p) < header.ARPSize {
			return false
		}
		a := header.ARP(p)
		return a.IsValid() && tcpip.AddrFrom4Slice(a.ProtocolAddressTarget()) == ourLL &&
			tcpip.AddrFrom4Slice(a.ProtocolAddressSender()) == g
	case header.IPv4ProtocolNumber:
		if len(p) < header.IPv4MinimumSize {
			return false
		}
		ip := header.IPv4(p)
		if !ip.IsValid(len(p)) || ip.HeaderLength() != header.IPv4MinimumSize || ip.More() || ip.FragmentOffset() != 0 ||
			ip.SourceAddress() != g || ip.DestinationAddress() != ourLL || ip.TransportProtocol() != header.TCPProtocolNumber {
			return false
		}
		t := header.TCP(ip.Payload())
		if len(t) < header.TCPMinimumSize || t.SourcePort() != 22 {
			return false
		}
		k.mu.Lock()
		open := k.ports[t.DestinationPort()]
		k.mu.Unlock()
		return open
	}
	return false
}

// linkLocal is the computer's self-assigned address, when the frame is an ARP from it.
func linkLocal(f []byte) (tcpip.Address, bool) {
	if len(f) < header.EthernetMinimumSize+header.ARPSize || header.Ethernet(f).Type() != header.ARPProtocolNumber {
		return tcpip.Address{}, false
	}
	a := header.ARP(f[header.EthernetMinimumSize:])
	src := a.ProtocolAddressSender()
	if !a.IsValid() || src[0] != 169 || src[1] != 254 || tcpip.AddrFrom4Slice(src) == ourLL {
		return tcpip.Address{}, false
	}
	return tcpip.AddrFrom4Slice(src), true
}

// card is the Mac end of one computer's card: what it does with each frame the computer sends.
type card struct {
	c      *counter
	router net.IP
	s      *stack.Stack // nil unless the way in is open
	ch     *channel.Endpoint
	send   func([]byte) // a frame to the computer
	guest  chan tcpip.Address

	mu        sync.Mutex
	learned   bool
	guestAddr tcpip.Address
	ports     map[uint16]bool // the Mac end's ports with a connection open or opening
	next      uint16
}

// newStack is the netstack of the way in, with the Mac end's own address on its one card.
func newStack() (*stack.Stack, *channel.Endpoint) {
	ch := channel.New(256, 1500, tcpip.LinkAddress(ourMAC))
	s := stack.New(stack.Options{
		NetworkProtocols:   []stack.NetworkProtocolFactory{ipv4.NewProtocol, arp.NewProtocol},
		TransportProtocols: []stack.TransportProtocolFactory{tcp.NewProtocol},
	})
	if e := s.CreateNIC(1, ethernet.New(ch)); e != nil {
		log.Fatal(e)
	}
	// The Mac end's address alone, and no route until main's address is learned: then one route,
	// to main's address only. The stack can send to nothing else.
	s.AddProtocolAddress(1, tcpip.ProtocolAddress{Protocol: ipv4.ProtocolNumber,
		AddressWithPrefix: tcpip.AddressWithPrefix{Address: ourLL, PrefixLen: 32}}, stack.AddressProperties{})
	// No SACK: the card loses no frames, and it is code main's segments would reach for nothing.
	sack := tcpip.TCPSACKEnabled(false)
	s.SetTransportProtocolOption(tcp.ProtocolNumber, &sack)
	return s, ch
}

// pump sends every frame the stack writes to the computer, until the stack closes.
func (k *card) pump() {
	for {
		pkt := k.ch.ReadContext(context.Background())
		if pkt == nil {
			return
		}
		v := pkt.ToView()
		k.send(v.AsSlice())
		v.Release()
		pkt.DecRef()
	}
}

// frame handles one frame from the computer.
func (k *card) frame(f []byte) {
	if r := arpReply(f, k.router); r != nil {
		k.c.add("arp who-has router, answered")
		k.send(r)
		return
	}
	if k.s != nil {
		k.mu.Lock()
		learned := k.learned
		k.mu.Unlock()
		if a, ok := linkLocal(f); ok && !learned {
			sub, _ := tcpip.NewSubnet(a, tcpip.MaskFromBytes(net.CIDRMask(32, 32)))
			k.s.SetRouteTable([]tcpip.Route{{Destination: sub, NIC: 1}})
			k.mu.Lock()
			k.learned, k.guestAddr = true, a
			k.mu.Unlock()
			k.guest <- a
		}
	}
	if k.s != nil && k.forStack(f) {
		k.c.add("for the way in")
		pkt := stack.NewPacketBuffer(stack.PacketBufferOptions{Payload: buffer.MakeWithData(append([]byte(nil), f...))})
		k.ch.InjectInbound(0, pkt)
		pkt.DecRef()
		return
	}
	k.c.add(classify(f))
}

// dial is one TCP connection from the Mac end to the computer's port 22. Its local port is chosen
// here and opened in the filter before the SYN leaves, and closed with the connection.
func (k *card) dial(g tcpip.Address) (net.Conn, error) {
	k.mu.Lock()
	if k.ports == nil {
		k.ports = map[uint16]bool{}
	}
	var port uint16
	for {
		k.next++
		port = 49152 + k.next%16384
		if !k.ports[port] {
			break
		}
	}
	k.ports[port] = true
	k.mu.Unlock()
	c, err := gonet.DialTCPWithBind(context.Background(), k.s,
		tcpip.FullAddress{NIC: 1, Addr: ourLL, Port: port}, tcpip.FullAddress{NIC: 1, Addr: g, Port: 22}, ipv4.ProtocolNumber)
	if err != nil {
		k.closePort(port)
		return nil, err
	}
	return &portConn{TCPConn: c, close: func() { k.closePort(port) }}, nil
}

func (k *card) closePort(p uint16) {
	k.mu.Lock()
	delete(k.ports, p)
	k.mu.Unlock()
}

// portConn closes its port in the filter when it closes. Frames still in flight for it after
// that are dropped and counted, which is what the stack would answer them with anyway: nothing
// main needs.
type portConn struct {
	*gonet.TCPConn
	once  sync.Once
	close func()
}

func (c *portConn) Close() error {
	err := c.TCPConn.Close()
	c.once.Do(c.close)
	return err
}

func main() {
	frames := flag.String("frames", "", "unix datagram socket the computer's frames arrive on")
	counts := flag.String("counts", "counts.json", "file the dropped frames are counted in")
	dial := flag.String("dial", "", "unix socket where each connection becomes one TCP connection to the computer's port 22")
	router := flag.String("arp-router", "10.254.0.1", "the one IPv4 address answered for")
	flag.Parse()

	c := &counter{kinds: map[string]int{}, ports: map[string][]string{}, path: *counts}
	os.Remove(*frames)
	conn, err := net.ListenUnixgram("unixgram", &net.UnixAddr{Name: *frames, Net: "unixgram"})
	if err != nil {
		log.Fatal(err)
	}
	conn.SetReadBuffer(4 << 20)
	var (
		peerMu sync.Mutex
		peer   *net.UnixAddr
	)
	k := &card{c: c, router: net.ParseIP(*router), guest: make(chan tcpip.Address, 1)}
	k.send = func(b []byte) {
		peerMu.Lock()
		p := peer
		peerMu.Unlock()
		if p != nil {
			conn.WriteToUnix(b, p)
		}
	}
	if *dial != "" {
		k.s, k.ch = newStack()
		go k.pump()
	}

	go func() {
		buf := make([]byte, 65536) // a longer datagram is cut, and dropped by forStack as too long
		for {
			n, from, err := conn.ReadFromUnix(buf)
			if err != nil {
				log.Fatal(err)
			}
			peerMu.Lock()
			peer = from
			peerMu.Unlock()
			f := buf[:n]
			if n == 4 && string(f) == "VFKT" { // the stand-in's handshake; a Mac's socket pair has none
				continue
			}
			k.frame(f)
		}
	}()

	if *dial != "" {
		go func() {
			g := <-k.guest
			log.Printf("computer at [%v]", g)
			os.Remove(*dial)
			l, err := net.Listen("unix", *dial)
			if err != nil {
				log.Fatal(err)
			}
			for {
				a, err := l.Accept()
				if err != nil {
					log.Fatal(err)
				}
				go func(a net.Conn) {
					defer a.Close()
					t, err := k.dial(g)
					if err != nil {
						log.Printf("dial: %v", err)
						return
					}
					defer t.Close()
					done := make(chan struct{}, 2)
					go func() { io.Copy(t, a); t.(*portConn).CloseWrite(); done <- struct{}{} }()
					go func() { io.Copy(a, t); done <- struct{}{} }()
					<-done
					<-done
				}(a)
			}
		}()
	}

	tick := time.NewTicker(500 * time.Millisecond)
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGTERM, syscall.SIGINT)
	for {
		select {
		case <-tick.C:
			c.write()
		case <-sig:
			c.write()
			return
		}
	}
}
