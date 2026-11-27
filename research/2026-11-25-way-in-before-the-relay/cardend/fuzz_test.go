package main

import (
	"encoding/binary"
	"net"
	"testing"
	"time"

	"gvisor.dev/gvisor/pkg/tcpip"
	"gvisor.dev/gvisor/pkg/tcpip/header"
)

// checkOut fails when the card sent main anything but ARP or TCP from the card's address to main's.
func checkOut(t *testing.T, frames [][]byte) {
	for _, f := range frames {
		if len(f) < 14 {
			t.Fatalf("sent a runt of %d bytes", len(f))
		}
		switch binary.BigEndian.Uint16(f[12:]) {
		case 0x0806:
		case 0x0800:
			ip := header.IPv4(f[14:])
			if !ip.IsValid(len(f)-14) || ip.Protocol() != 6 || ip.SourceAddress() != ourLL || ip.DestinationAddress() != tcpip.AddrFrom4Slice(mainLL) {
				t.Fatalf("sent %s", describe(f))
			}
		default:
			t.Fatalf("sent %s", describe(f))
		}
	}
}

// FuzzFrame: any frame main sends, to a card that has learned main and has no connection open.
// The card must not panic, must send nothing but ARP or TCP to main, and its count must stay small.
func FuzzFrame(f *testing.F) {
	f.Add(announce())
	f.Add(ethf(bcast, 0x0806, arpf(1, mainMAC, mainLL, make([]byte, 6), []byte{10, 254, 0, 1})))
	f.Add(ethf(bcast, 0x0806, arpf(1, mainMAC, mainLL, make([]byte, 6), cardLL)))
	f.Add(ethf(bcast, 0x0800, ip4(mainLL, cardLL, 6, 0, nil, tcpf(mainLL, cardLL, 22, 40000, fSYN|fACK, 1, 1, 65535, []byte{2, 4, 5, 0xb4, 4, 2, 1, 1}, nil))))
	f.Add(ethf(bcast, 0x0800, ip4(mainLL, cardLL, 6, 0x2000, []byte{1, 1, 1, 0}, tcpf(mainLL, cardLL, 22, 40000, fACK, 1, 1, 65535, nil, make([]byte, 40)))))
	f.Add(ethf(bcast, 0x0800, ip4(mainLL, []byte{8, 8, 8, 8}, 17, 0, nil, make([]byte, 40))))
	f.Add(ethf(bcast, 0x86dd, make([]byte, 60)))
	f.Add([]byte{1, 2, 3})
	r := newRig(f)
	f.Fuzz(func(t *testing.T, b []byte) {
		r.mu.Lock()
		r.out = r.out[:0]
		r.mu.Unlock()
		r.k.frame(b)
		checkOut(t, r.sent())
		r.k.c.mu.Lock()
		n := len(r.k.c.kinds)
		r.k.c.mu.Unlock()
		if n > 300 {
			t.Fatalf("the count holds %d kinds", n)
		}
	})
}

// FuzzConnection: main as the TCP peer of the way in, sending any segments it likes on the
// connection's own addresses and ports while Silo writes to it. Each input is a list of segments:
// flags, seq and ack offsets, window, then options and data of the lengths given.
func FuzzConnection(f *testing.F) {
	f.Add([]byte{fACK | fPSH, 0, 0, 0, 0, 0xff, 0xff, 0, 4, 'h', 'e', 'l', 'o'})
	f.Add([]byte{fACK, 0, 10, 0, 0, 0, 0, 0, 0, fRST, 0, 0, 0, 0, 0, 0, 0, 0})
	f.Add([]byte{fACK, 0, 0, 0, 0, 0xff, 0xff, 10, 0, 5, 10, 0, 0, 0, 1, 0, 0, 0, 2, 1, 1})
	f.Add([]byte{fSYN, 0, 0, 0, 0, 1, 0, 4, 0, 2, 4, 0, 1})
	f.Add([]byte{fFIN | fACK, 0, 0, 0, 0, 1, 0, 0, 0})
	f.Fuzz(func(t *testing.T, b []byte) {
		r := newRig(t)
		type res struct {
			c   net.Conn
			err error
		}
		done := make(chan res, 1)
		go func() { c, err := r.k.dial(tcpip.AddrFrom4Slice(mainLL)); done <- res{c, err} }()
		// The card ARPs for main, then sends its SYN.
		var synF []byte
		for deadline := time.Now().Add(2 * time.Second); synF == nil && time.Now().Before(deadline); {
			for _, o := range r.sent() {
				if binary.BigEndian.Uint16(o[12:]) == 0x0806 {
					a := header.ARP(o[14:])
					if a.Op() == header.ARPRequest {
						r.k.frame(ethf(o[6:12], 0x0806, arpf(2, mainMAC, mainLL, a.HardwareAddressSender(), cardLL)))
					}
				} else if header.IPv4(o[14:]).Protocol() == 6 && header.TCP(header.IPv4(o[14:]).Payload()).Flags()&header.TCPFlagSyn != 0 {
					synF = o
				}
			}
			time.Sleep(time.Millisecond)
		}
		if synF == nil {
			t.Fatal("no SYN")
		}
		syn := header.TCP(header.IPv4(synF[14:]).Payload())
		port, iss := syn.SourcePort(), syn.SequenceNumber()
		const irs = 1000
		r.k.frame(ethf(ourMAC, 0x0800, ip4(mainLL, cardLL, 6, 0, nil, tcpf(mainLL, cardLL, 22, port, fSYN|fACK, irs, iss+1, 65535, nil, nil))))
		d := <-done
		if d.err != nil {
			t.Fatal(d.err)
		}
		c := d.c
		go func() { c.Write(make([]byte, 100000)) }()
		go func() { c.Read(make([]byte, 100000)) }()
		r.mu.Lock()
		r.out = r.out[:0]
		r.mu.Unlock()
		for len(b) >= 9 {
			flags := b[0]
			seq := uint32(irs+1) + uint32(int16(binary.BigEndian.Uint16(b[1:])))
			ack := iss + 1 + uint32(int16(binary.BigEndian.Uint16(b[3:])))
			win := binary.BigEndian.Uint16(b[5:])
			no, nd := int(b[7]%41), int(b[8])
			b = b[9:]
			if no > len(b) {
				no = len(b)
			}
			opts := b[:no]
			b = b[no:]
			if nd > len(b) {
				nd = len(b)
			}
			data := b[:nd]
			b = b[nd:]
			r.k.frame(ethf(ourMAC, 0x0800, ip4(mainLL, cardLL, 6, 0, nil, tcpf(mainLL, cardLL, 22, port, flags, seq, ack, win, opts, data))))
		}
		time.Sleep(2 * time.Millisecond)
		c.Close()
		checkOut(t, r.sent())
	})
}
