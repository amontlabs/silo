import socket, fcntl, struct
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
ifr = struct.pack("16sh", b"lo", 0)
flags = struct.unpack("16sh", fcntl.ioctl(s, 0x8913, ifr))[1]
fcntl.ioctl(s, 0x8914, struct.pack("16sh", b"lo", flags | 0x1))
