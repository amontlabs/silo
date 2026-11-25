// standin-boundary plays the part of Silo's boundary behind gvproxy's -outbound-handoff. It takes
// each TCP connection gvproxy hands it, reads the PROXY v2 header, logs the destination, refuses
// what its list refuses, and otherwise dials the destination and copies bytes both ways. It also
// answers name lookups (-dns), so the host's resolver never sees one.
//
// usage: standin-boundary -sock <path> -log <file> [-dns 127.0.0.1:5353] [-deny-private]
//        [-answer name=ip ...] [-sink]   (-sink: reply to nothing, read and drop: for throughput)
package main

import (
	"encoding/binary"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"strings"
	"sync"
	"syscall"
)

var (
	logMu   sync.Mutex
	logFile *os.File
)

func record(format string, a ...any) {
	logMu.Lock()
	defer logMu.Unlock()
	fmt.Fprintf(logFile, format+"\n", a...)
}

func readHeader(c net.Conn) (src, dst *net.TCPAddr, err error) {
	h := make([]byte, 16)
	if _, err = io.ReadFull(c, h); err != nil {
		return
	}
	if string(h[:12]) != "\r\n\r\n\x00\r\nQUIT\n" || h[12] != 0x21 {
		return nil, nil, fmt.Errorf("not a PROXY v2 header")
	}
	body := make([]byte, binary.BigEndian.Uint16(h[14:]))
	if _, err = io.ReadFull(c, body); err != nil {
		return
	}
	n := 4
	if h[13] == 0x21 {
		n = 16
	}
	src = &net.TCPAddr{IP: net.IP(body[:n]), Port: int(binary.BigEndian.Uint16(body[2*n:]))}
	dst = &net.TCPAddr{IP: net.IP(body[n : 2*n]), Port: int(binary.BigEndian.Uint16(body[2*n+2:]))}
	return
}

func private(ip net.IP) bool {
	return ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() || ip.IsUnspecified() || ip.IsMulticast()
}

func main() {
	sock := flag.String("sock", "", "unix socket gvproxy hands connections to")
	logPath := flag.String("log", "/dev/stderr", "where each connection and name is logged")
	dnsAddr := flag.String("dns", "", "udp address to answer name lookups on")
	denyPrivate := flag.Bool("deny-private", true, "refuse private, loopback and link-local destinations")
	sink := flag.Bool("sink", false, "read and drop every byte, dial nothing")
	var answers multi
	flag.Var(&answers, "answer", "name=ip, the A record the boundary gives for name")
	flag.Parse()
	var err error
	if logFile, err = os.OpenFile(*logPath, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644); err != nil {
		log.Fatal(err)
	}
	if *dnsAddr != "" {
		go serveDNS(*dnsAddr, answers)
	}
	os.Remove(*sock)
	ln, err := net.Listen("unix", *sock)
	if err != nil {
		log.Fatal(err)
	}
	for {
		c, err := ln.Accept()
		if err != nil {
			log.Fatal(err)
		}
		go func() {
			defer c.Close()
			src, dst, err := readHeader(c)
			if err != nil {
				record("bad hand-off: %v", err)
				return
			}
			if *sink {
				io.Copy(io.Discard, c)
				return
			}
			if *denyPrivate && private(dst.IP) {
				record("tcp %s -> %s refused", src, dst)
				return
			}
			out, err := net.DialTCP("tcp", nil, dst)
			if err != nil {
				record("tcp %s -> %s unreachable", src, dst)
				return
			}
			defer out.Close()
			out.SetNoDelay(true)
			record("tcp %s -> %s", src, dst)
			done := make(chan struct{}, 2)
			go func() { io.Copy(out, c); out.CloseWrite(); done <- struct{}{} }()
			go func() {
				io.Copy(c, out)
				if u, ok := c.(*net.UnixConn); ok {
					u.CloseWrite()
				}
				done <- struct{}{}
			}()
			<-done
			<-done
		}()
	}
}

type multi []string

func (m *multi) String() string     { return strings.Join(*m, ",") }
func (m *multi) Set(v string) error { *m = append(*m, v); return nil }

// serveDNS answers A queries for the names given and NXDOMAIN for every other, logging each.
func serveDNS(addr string, answers multi) {
	table := map[string]net.IP{}
	for _, a := range answers {
		k, v, _ := strings.Cut(a, "=")
		table[strings.ToLower(strings.TrimSuffix(k, "."))] = net.ParseIP(v).To4()
	}
	pc, err := net.ListenPacket("udp", addr)
	if err != nil {
		log.Fatal(err)
	}
	buf := make([]byte, 4096)
	for {
		n, peer, err := pc.ReadFrom(buf)
		if err != nil || n < 17 {
			continue
		}
		d := buf[:n]
		i, labels := 12, []string{}
		for i < n && d[i] != 0 && i+1+int(d[i]) <= n {
			labels = append(labels, string(d[i+1:i+1+int(d[i])]))
			i += 1 + int(d[i])
		}
		if i+5 > n {
			continue
		}
		name := strings.ToLower(strings.Join(labels, "."))
		qtype := binary.BigEndian.Uint16(d[i+1:])
		q := d[12 : i+5]
		record("dns %s %d", name, qtype)
		ip, ok := table[name]
		var r []byte
		if ok && qtype == 1 {
			r = append(append([]byte{d[0], d[1], 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0}, q...), 0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4)
			r = append(r, ip...)
		} else if ok {
			r = append([]byte{d[0], d[1], 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0}, q...)
		} else {
			r = append([]byte{d[0], d[1], 0x81, 0x83, 0, 1, 0, 0, 0, 0, 0, 0}, q...)
		}
		pc.WriteTo(r, peer)
	}
}

var _ = syscall.SIGTERM
