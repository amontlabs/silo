package main

import (
	"encoding/binary"
	"math/rand/v2"
)

// Frames as a hostile main would send them.

var (
	mainMAC = []byte{0x5a, 0x94, 0xef, 0x00, 0x00, 0x77}
	mainLL  = []byte{169, 254, 77, 7}
	cardLL  = []byte{169, 254, 81, 16}
	bcast   = []byte{0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
)

func ethf(dst []byte, typ uint16, p []byte) []byte {
	f := make([]byte, 14+len(p))
	copy(f, dst)
	copy(f[6:], mainMAC)
	binary.BigEndian.PutUint16(f[12:], typ)
	copy(f[14:], p)
	return f
}

func arpf(op uint16, sha, spa, tha, tpa []byte) []byte {
	p := make([]byte, 28)
	binary.BigEndian.PutUint16(p[0:], 1)
	binary.BigEndian.PutUint16(p[2:], 0x0800)
	p[4], p[5] = 6, 4
	binary.BigEndian.PutUint16(p[6:], op)
	copy(p[8:], sha)
	copy(p[14:], spa)
	copy(p[18:], tha)
	copy(p[24:], tpa)
	return p
}

func sum16(b []byte, s uint32) uint16 {
	for i := 0; i+1 < len(b); i += 2 {
		s += uint32(binary.BigEndian.Uint16(b[i:]))
	}
	if len(b)%2 == 1 {
		s += uint32(b[len(b)-1]) << 8
	}
	for s>>16 != 0 {
		s = s&0xffff + s>>16
	}
	return ^uint16(s)
}

// ip4 builds an IPv4 packet with the given flags+offset field and options.
func ip4(src, dst []byte, proto byte, frag uint16, opts []byte, payload []byte) []byte {
	hl := 20 + len(opts)
	h := make([]byte, hl+len(payload))
	h[0] = 0x40 | byte(hl/4)
	binary.BigEndian.PutUint16(h[2:], uint16(len(h)))
	binary.BigEndian.PutUint16(h[4:], uint16(rand.Uint32()))
	binary.BigEndian.PutUint16(h[6:], frag)
	h[8] = 64
	h[9] = proto
	copy(h[12:], src)
	copy(h[16:], dst)
	copy(h[20:], opts)
	binary.BigEndian.PutUint16(h[10:], sum16(h[:hl], 0))
	copy(h[hl:], payload)
	return h
}

// tcpf builds a TCP segment with a correct checksum.
func tcpf(src, dst []byte, sport, dport uint16, flags byte, seq, ack uint32, win uint16, opts, data []byte) []byte {
	hl := 20 + (len(opts)+3)/4*4
	t := make([]byte, hl+len(data))
	binary.BigEndian.PutUint16(t[0:], sport)
	binary.BigEndian.PutUint16(t[2:], dport)
	binary.BigEndian.PutUint32(t[4:], seq)
	binary.BigEndian.PutUint32(t[8:], ack)
	t[12] = byte(hl/4) << 4
	t[13] = flags
	binary.BigEndian.PutUint16(t[14:], win)
	copy(t[20:], opts)
	copy(t[hl:], data)
	ph := make([]byte, 12)
	copy(ph, src)
	copy(ph[4:], dst)
	ph[9] = 6
	binary.BigEndian.PutUint16(ph[10:], uint16(len(t)))
	var s uint32
	for i := 0; i < 12; i += 2 {
		s += uint32(binary.BigEndian.Uint16(ph[i:]))
	}
	binary.BigEndian.PutUint16(t[16:], sum16(t, s))
	return t
}

func rnd4() []byte {
	var b [4]byte
	binary.BigEndian.PutUint32(b[:], rand.Uint32())
	return b[:]
}

const (
	fFIN = 0x01
	fSYN = 0x02
	fRST = 0x04
	fPSH = 0x08
	fACK = 0x10
)

// announce is main telling the card its self-assigned address, as macOS does.
func announce() []byte { return ethf(bcast, 0x0806, arpf(1, mainMAC, mainLL, make([]byte, 6), mainLL)) }
