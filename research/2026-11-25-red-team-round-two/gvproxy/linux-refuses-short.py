# Can root in a Linux computer put a frame shorter than an Ethernet header on its card?
# A TAP device stands for the card: what the computer's kernel sends is what the card's far end reads.
import fcntl, os, socket, struct, select
TUNSETIFF, IFF_TAP, IFF_NO_PI = 0x400454ca, 0x0002, 0x1000
fd = os.open("/dev/net/tun", os.O_RDWR)
fcntl.ioctl(fd, TUNSETIFF, struct.pack("16sH", b"silotap0", IFF_TAP | IFF_NO_PI))
c = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
flags = struct.unpack("16sH", fcntl.ioctl(c, 0x8913, struct.pack("16sH", b"silotap0", 0)))[1]
fcntl.ioctl(c, 0x8914, struct.pack("16sH", b"silotap0", flags | 1))
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
s.bind(("silotap0", 0))
for frame in (b"VFKT", b"\xff" * 6, b"\xff" * 12, b"\xff" * 14):  # the last one shows the path works
    try:
        s.send(frame)
    except OSError as e:
        print("%-12r refused by the kernel: %s" % (frame, e)); continue
    while True:
        r, _, _ = select.select([fd], [], [], 1)
        if not r: print("%-12r sent, nothing reached the card's back end" % frame); break
        got = os.read(fd, 65536)
        if got == frame: print("%-12r reached the card's back end as %d bytes" % (frame, len(got))); break
