// fwdgram: fwd.go's part, over a unix datagram socket, one frame per datagram, as Apple's
// VZFileHandleNetworkDeviceAttachment and vfkit hand frames to gvproxy's -listen-vfkit.
// usage: fwdgram <tap> <gvproxy-socket>
package main

import (
	"net"
	"os"
	"unsafe"

	"golang.org/x/sys/unix"
)

func tap(name string) *os.File {
	fd, err := unix.Open("/dev/net/tun", unix.O_RDWR, 0)
	if err != nil {
		panic(err)
	}
	var req [40]byte
	copy(req[:16], name)
	*(*uint16)(unsafe.Pointer(&req[16])) = unix.IFF_TAP | unix.IFF_NO_PI
	if _, _, e := unix.Syscall(unix.SYS_IOCTL, uintptr(fd), unix.TUNSETIFF, uintptr(unsafe.Pointer(&req[0]))); e != 0 {
		panic(e)
	}
	return os.NewFile(uintptr(fd), name)
}

func main() {
	t := tap(os.Args[1])
	local := os.Args[2] + ".client"
	os.Remove(local)
	c, err := net.DialUnix("unixgram", &net.UnixAddr{Name: local, Net: "unixgram"}, &net.UnixAddr{Name: os.Args[2], Net: "unixgram"})
	if err != nil {
		panic(err)
	}
	c.SetWriteBuffer(1 << 20)
	c.SetReadBuffer(4 << 20)
	if _, err := c.Write([]byte("VFKT")); err != nil {
		panic(err)
	}
	go func() {
		buf := make([]byte, 65536)
		for {
			n, err := t.Read(buf)
			if err != nil {
				panic(err)
			}
			c.Write(buf[:n]) // a full socket buffer drops the frame, as a real link would
		}
	}()
	buf := make([]byte, 65536)
	for {
		n, err := c.Read(buf)
		if err != nil {
			panic(err)
		}
		t.Write(buf[:n])
	}
}
