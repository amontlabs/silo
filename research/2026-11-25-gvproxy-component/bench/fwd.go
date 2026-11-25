// fwd: plays the virtual machine monitor's part. Opens a tap device (in the current network
// namespace) and carries its frames to a user-space network stack over a unix stream socket,
// with the QEMU framing (4-byte big-endian length, then the frame). passt and gvproxy both speak it.
package main

import (
	"encoding/binary"
	"io"
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
	c, err := net.Dial("unix", os.Args[2])
	if err != nil {
		panic(err)
	}
	go func() { // tap -> stack
		buf := make([]byte, 65540)
		for {
			n, err := t.Read(buf[4:])
			if err != nil {
				panic(err)
			}
			binary.BigEndian.PutUint32(buf, uint32(n))
			if _, err := c.Write(buf[:n+4]); err != nil {
				panic(err)
			}
		}
	}()
	hdr := make([]byte, 4)
	buf := make([]byte, 65536)
	for { // stack -> tap
		if _, err := io.ReadFull(c, hdr); err != nil {
			panic(err)
		}
		n := binary.BigEndian.Uint32(hdr)
		if _, err := io.ReadFull(c, buf[:n]); err != nil {
			panic(err)
		}
		t.Write(buf[:n])
	}
}
