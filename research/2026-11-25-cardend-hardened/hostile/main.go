// hostile: a hostile main, sending frames to cardend's socket and counting what comes back.
//
//	hostile -card <cardend's frames socket> -mode <mode> -n <frames> [-pid <cardend's pid>]
//
// It first announces main at 169.254.77.7 by ARP, as macOS does, so cardend learns the address.
// Then it sends n frames of one kind as fast as the socket takes them, drains every frame cardend
// sends back, and prints one JSON line: frames sent, frames back, and with -pid cardend's resident
// memory and CPU time before and after.
package main

import (
	"encoding/binary"
	"encoding/json"
	"flag"
	"fmt"
	"math/rand/v2"
	"net"
	"os"
	"strconv"
	"strings"
	"sync/atomic"
	"time"
)

var (
	mainMAC = []byte{0x5a, 0x94, 0xef, 0x00, 0x00, 0x77}
	cardMAC = []byte{0x5a, 0x94, 0xef, 0x00, 0x00, 0x01}
	mainLL  = []byte{169, 254, 77, 7}
	cardLL  = []byte{169, 254, 81, 16}
	bcast   = []byte{0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
)

func eth(dst []byte, typ uint16, p []byte) []byte {
	f := make([]byte, 14+len(p))
	copy(f, dst)
	copy(f[6:], mainMAC)
	binary.BigEndian.PutUint16(f[12:], typ)
	copy(f[14:], p)
	return f
}

func arp(op uint16, sha, spa, tha, tpa []byte) []byte {
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

func csum(b []byte, init uint32) uint16 {
	s := init
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

// ipv4 builds a packet; frag is the flags+offset field.
func ipv4(src, dst []byte, proto byte, id uint16, frag uint16, payload []byte) []byte {
	h := make([]byte, 20+len(payload))
	h[0] = 0x45
	binary.BigEndian.PutUint16(h[2:], uint16(len(h)))
	binary.BigEndian.PutUint16(h[4:], id)
	binary.BigEndian.PutUint16(h[6:], frag)
	h[8] = 64
	h[9] = proto
	copy(h[12:], src)
	copy(h[16:], dst)
	binary.BigEndian.PutUint16(h[10:], csum(h[:20], 0))
	copy(h[20:], payload)
	return h
}

func tcp(src, dst []byte, sport, dport uint16, flags byte, seq uint32, data []byte) []byte {
	t := make([]byte, 20+len(data))
	binary.BigEndian.PutUint16(t[0:], sport)
	binary.BigEndian.PutUint16(t[2:], dport)
	binary.BigEndian.PutUint32(t[4:], seq)
	t[12] = 5 << 4
	t[13] = flags
	binary.BigEndian.PutUint16(t[14:], 65535)
	copy(t[20:], data)
	ph := make([]byte, 12)
	copy(ph, src)
	copy(ph[4:], dst)
	ph[9] = 6
	binary.BigEndian.PutUint16(ph[10:], uint16(len(t)))
	var s uint32
	for i := 0; i < 12; i += 2 {
		s += uint32(binary.BigEndian.Uint16(ph[i:]))
	}
	binary.BigEndian.PutUint16(t[16:], csum(t, s))
	return t
}

func rnd4() []byte { var b [4]byte; binary.BigEndian.PutUint32(b[:], rand.Uint32()); return b[:] }

const syn, ack = 0x02, 0x10

// frame returns the i-th frame of a mode.
func frame(mode string, i int) []byte {
	switch mode {
	case "same": // one kind, again and again: the cost of a frame
		return eth(bcast, 0x0800, ipv4(mainLL, []byte{8, 8, 8, 8}, 17, uint16(i), 0, make([]byte, 8+32)))
	case "arp-spread": // ARP for a new address each time
		return eth(bcast, 0x0806, arp(1, mainMAC, mainLL, make([]byte, 6), rnd4()))
	case "tcp-spread": // a SYN to a new address each time
		d := rnd4()
		return eth(cardMAC, 0x0800, ipv4(mainLL, d, 6, uint16(i), 0, tcp(mainLL, d, 40000, uint16(rand.Uint32()), syn, 1, nil)))
	case "icmp-spread":
		return eth(cardMAC, 0x0800, ipv4(mainLL, rnd4(), 1, uint16(i), 0, []byte{8, 0, 0, 0, 0, 0, 0, 0}))
	case "v6-spread": // a SYN to a new IPv6 address each time
		p := make([]byte, 40+20)
		p[0] = 0x60
		binary.BigEndian.PutUint16(p[4:], 20)
		p[6], p[7] = 6, 64
		for j := 24; j < 40; j += 4 {
			copy(p[j:], rnd4())
		}
		binary.BigEndian.PutUint16(p[40:], 40000)
		binary.BigEndian.PutUint16(p[42:], 22)
		p[52] = 5 << 4
		p[53] = syn
		return eth(cardMAC, 0x86dd, p)
	case "syn22": // a SYN from port 22 to the card's own address, each to a new port
		return eth(cardMAC, 0x0800, ipv4(mainLL, cardLL, 6, uint16(i), 0, tcp(mainLL, cardLL, 22, uint16(1+i%65535), syn, 1, nil)))
	case "spoof22": // TCP from port 22, from a new 169.254 address each time
		s := []byte{169, 254, byte(rand.Uint32()), byte(rand.Uint32() | 1)}
		return eth(cardMAC, 0x0800, ipv4(s, cardLL, 6, uint16(i), 0, tcp(s, cardLL, 22, 40000, ack, 1, nil)))
	case "frag22": // first fragments that never complete: 1,400 bytes held each, a new id each time
		t := tcp(mainLL, cardLL, 22, 40000, ack, 1, make([]byte, 1380))
		return eth(cardMAC, 0x0800, ipv4(mainLL, cardLL, 6, uint16(i), 0x2000, t))
	case "big22": // a 64 KB datagram that claims to be one frame, from port 22
		t := tcp(mainLL, cardLL, 22, 40000, ack, 1, make([]byte, 65000))
		return eth(cardMAC, 0x0800, ipv4(mainLL, cardLL, 6, uint16(i), 0, t))
	case "arp-reply-spread": // ARP replies teaching the card a new neighbour each time
		return eth(cardMAC, 0x0806, arp(2, mainMAC, []byte{169, 254, byte(rand.Uint32()), byte(rand.Uint32() | 1)}, cardMAC, cardLL))
	}
	panic("unknown mode " + mode)
}

func procStats(pid int) (rssKB int, cpuMS int) {
	b, _ := os.ReadFile(fmt.Sprintf("/proc/%d/status", pid))
	for _, l := range strings.Split(string(b), "\n") {
		if strings.HasPrefix(l, "VmRSS:") {
			rssKB, _ = strconv.Atoi(strings.Fields(l)[1])
		}
	}
	s, _ := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid))
	f := strings.Fields(string(s[strings.LastIndexByte(string(s), ')')+2:]))
	u, _ := strconv.Atoi(f[11])
	k, _ := strconv.Atoi(f[12])
	return rssKB, (u + k) * 10
}

func main() {
	card := flag.String("card", "", "cardend's frames socket")
	mode := flag.String("mode", "same", "kind of frame")
	n := flag.Int("n", 100000, "frames to send")
	pid := flag.Int("pid", 0, "cardend's pid, to measure")
	settle := flag.Duration("settle", 2*time.Second, "wait after the last frame")
	dur := flag.Duration("for", 0, "send for this long instead of n frames, printing the rate every 5 s")
	flag.Parse()

	me := fmt.Sprintf("/tmp/hostile-%d.sock", os.Getpid())
	os.Remove(me)
	c, err := net.DialUnix("unixgram", &net.UnixAddr{Name: me, Net: "unixgram"}, &net.UnixAddr{Name: *card, Net: "unixgram"})
	if err != nil {
		panic(err)
	}
	defer os.Remove(me)
	c.SetWriteBuffer(8 << 20)
	var back, backBytes atomic.Int64
	go func() {
		b := make([]byte, 70000)
		for {
			m, err := c.Read(b)
			if err != nil {
				return
			}
			back.Add(1)
			backBytes.Add(int64(m))
		}
	}()
	c.Write(eth(bcast, 0x0806, arp(1, mainMAC, mainLL, make([]byte, 6), mainLL))) // announce
	time.Sleep(200 * time.Millisecond)
	back.Store(0)
	backBytes.Store(0)

	var r0, c0 int
	if *pid > 0 {
		r0, c0 = procStats(*pid)
	}
	t0 := time.Now()
	sent := 0
	last, lastT := 0, t0
	for i := 0; *dur > 0 || i < *n; i++ {
		if *dur > 0 {
			if now := time.Now(); now.Sub(lastT) >= 5*time.Second {
				fmt.Printf("{\"at_s\":%.0f,\"frames_per_s\":%.0f}\n", now.Sub(t0).Seconds(), float64(i-last)/now.Sub(lastT).Seconds())
				last, lastT = i, now
				if now.Sub(t0) >= *dur {
					break
				}
			}
		}
		if _, err := c.Write(frame(*mode, i)); err != nil {
			fmt.Fprintln(os.Stderr, "write:", err)
			break
		}
		sent++
	}
	el := time.Since(t0)
	time.Sleep(*settle)
	out := map[string]any{"mode": *mode, "sent": sent, "send_s": el.Seconds(), "back": back.Load(), "back_bytes": backBytes.Load()}
	if *pid > 0 {
		r1, c1 := procStats(*pid)
		out["rss_kb_before"], out["rss_kb_after"], out["cpu_ms"] = r0, r1, c1-c0
	}
	j, _ := json.Marshal(out)
	fmt.Println(string(j))
}
