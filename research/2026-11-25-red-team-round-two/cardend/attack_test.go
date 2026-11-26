// A hostile computer against cardend (#402). Put this file next to cardend's main.go and run
// `go test -run Attack -v .`. Each test is a frame, or a stream of frames, that root inside main
// can put on its network card.
package main

import (
	"encoding/binary"
	"os"
	"path/filepath"
	"testing"

	"gvisor.dev/gvisor/pkg/tcpip"
	"gvisor.dev/gvisor/pkg/tcpip/header"
)

// linesBound is what a fixed counter may hold: its lines, plus one "other" line per kind.
const linesBound = 256

var mainMAC = []byte{0x5a, 0x94, 0xef, 0x00, 0x00, 0x02}

// tcpFrame is an Ethernet frame holding one IPv4 TCP segment from main.
func tcpFrame(src, dst [4]byte, sport, dport uint16, flags header.TCPFlags, fragOff uint16, more bool) []byte {
	f := make([]byte, header.EthernetMinimumSize+header.IPv4MinimumSize+header.TCPMinimumSize)
	header.Ethernet(f).Encode(&header.EthernetFields{SrcAddr: tcpip.LinkAddress(mainMAC), DstAddr: tcpip.LinkAddress(ourMAC), Type: header.IPv4ProtocolNumber})
	ip := header.IPv4(f[header.EthernetMinimumSize:])
	var fl uint8
	if more {
		fl = header.IPv4FlagMoreFragments
	}
	ip.Encode(&header.IPv4Fields{TotalLength: uint16(header.IPv4MinimumSize + header.TCPMinimumSize), TTL: 64, Protocol: uint8(header.TCPProtocolNumber),
		SrcAddr: tcpip.AddrFrom4(src), DstAddr: tcpip.AddrFrom4(dst), Flags: fl, FragmentOffset: fragOff})
	ip.SetChecksum(^ip.CalculateChecksum())
	t := header.TCP(ip.Payload())
	t.Encode(&header.TCPFields{SrcPort: sport, DstPort: dport, SeqNum: 1, DataOffset: header.TCPMinimumSize, Flags: flags, WindowSize: 65535})
	return f
}

// What main can make the Mac end count. Every SYN to a new address, every ARP for a new address and
// every ICMP to a new address is a new line held in memory and written out twice a second. A scan
// of 10.0.0.0/8 from inside is 16.7 million lines.
func TestAttackCountsStayBounded(t *testing.T) {
	path := filepath.Join(t.TempDir(), "counts.json")
	c := &counter{kinds: map[string]int{}, ports: map[string][]string{}, path: path}
	const n = 100_000
	for i := 0; i < n; i++ {
		var dst [4]byte
		binary.BigEndian.PutUint32(dst[:], 0x0a000000+uint32(i)) // 10.0.0.0 and up
		c.add(classify(tcpFrame([4]byte{169, 254, 77, 7}, dst, 40000, 443, header.TCPFlagSyn, 0, false)))
	}
	c.write()
	st, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("%d SYNs to %d addresses: %d lines held, counts file %d bytes", n, n, len(c.kinds), st.Size())
	if len(c.kinds) > linesBound+16 {
		t.Errorf("the count holds %d lines; a computer decides how many", len(c.kinds))
	}
	if st.Size() > 64<<10 {
		t.Errorf("the counts file is %d bytes, rewritten twice a second", st.Size())
	}
	total := 0
	for _, v := range c.kinds {
		total += v
	}
	if total != n {
		t.Errorf("counted %d frames of %d: a bounded count must still count every frame", total, n)
	}
}

// #402 says "A SYN from main never reaches [the stack], so nothing in main can open a connection".
// The filter passes any TCP segment from port 22 to the Mac end, SYN included, and any fragment
// whose first bytes look like such a header.
func TestAttackStackSeesOnlyTheConnectionItOpened(t *testing.T) {
	g := [4]byte{169, 254, 77, 7}
	us := ourLL.As4()
	cases := []struct {
		name string
		f    []byte
		want bool
	}{
		{"SYN-ACK from 22, the answer to the Mac end's SYN", tcpFrame(g, us, 22, 50000, header.TCPFlagSyn|header.TCPFlagAck, 0, false), true},
		{"ACK with data from 22", tcpFrame(g, us, 22, 50000, header.TCPFlagAck|header.TCPFlagPsh, 0, false), true},
		{"bare SYN from 22: main opening a connection", tcpFrame(g, us, 22, 1, header.TCPFlagSyn, 0, false), false},
		{"first fragment, more to come", tcpFrame(g, us, 22, 50000, header.TCPFlagAck, 0, true), false},
		{"later fragment", tcpFrame(g, us, 22, 50000, header.TCPFlagAck, 8, false), false},
	}
	for _, tc := range cases {
		if got := forStack(tc.f); got != tc.want {
			t.Errorf("%s: handed to the stack = %v, want %v", tc.name, got, tc.want)
		}
	}
}
