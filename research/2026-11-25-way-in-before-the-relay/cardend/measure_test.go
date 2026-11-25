package main

import (
	"encoding/binary"
	"fmt"
	"io"
	"math/rand/v2"
	"os"
	"testing"
	"time"

	"gvisor.dev/gvisor/pkg/tcpip"
)

// TestMeasureWayInUnderFlood is a measurement, not a check: SSH's round trip through the way in
// while main floods the card with frames to a new IPv6 address each, through one frame reader
// and the half-second count write, as in main(). CARDEND_MEASURE=<seconds> runs it.
func TestMeasureWayInUnderFlood(t *testing.T) {
	secs := os.Getenv("CARDEND_MEASURE")
	if secs == "" {
		t.Skip("CARDEND_MEASURE=<seconds> runs it")
	}
	var dur time.Duration
	fmt.Sscan(secs, &dur)
	dur *= time.Second
	r := newRig(t)
	r.feed = make(chan []byte, 256)
	go func() {
		for f := range r.feed {
			r.k.frame(f)
		}
	}()
	r.mainStack(t)
	stop := make(chan struct{})
	go func() { // the count write of main()
		for {
			select {
			case <-stop:
				return
			case <-time.After(500 * time.Millisecond):
				r.k.c.write()
			}
		}
	}()
	c, err := r.k.dial(tcpip.AddrFrom4Slice(mainLL))
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	go func() { // the flood
		p := make([]byte, 60)
		p[0] = 0x60
		binary.BigEndian.PutUint16(p[4:], 20)
		p[6], p[7] = 6, 64
		for {
			select {
			case <-stop:
				return
			default:
			}
			q := append([]byte(nil), p...)
			for j := 24; j < 40; j += 4 {
				binary.BigEndian.PutUint32(q[j:], rand.Uint32())
			}
			q[53] = fSYN
			r.feed <- ethf(bcast, 0x86dd, q)
		}
	}()
	msg := make([]byte, 1024)
	back := make([]byte, 1024)
	t0 := time.Now()
	for time.Since(t0) < dur {
		s := time.Now()
		c.SetDeadline(s.Add(30 * time.Second))
		c.Write(msg)
		_, err := io.ReadFull(c, back)
		rtt := time.Since(s)
		r.k.c.mu.Lock()
		n := len(r.k.c.kinds)
		r.k.c.mu.Unlock()
		fmt.Printf("{\"at_s\":%.0f,\"ssh_round_trip_ms\":%d,\"count_lines\":%d,\"err\":%q}\n", time.Since(t0).Seconds(), rtt.Milliseconds(), n, fmt.Sprint(err))
		time.Sleep(time.Second - min(rtt, time.Second))
	}
	close(stop)
}
