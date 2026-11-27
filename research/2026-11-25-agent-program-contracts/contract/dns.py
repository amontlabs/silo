"""A name server for the contract's namespace: every name a program looks up points at the
boundary, and each lookup is written to OUT/lookups.txt. So a release that starts talking to a new
host shows it: the boundary refuses a host it does not read, and the lookup names it.

Environment: OUT, ANSWER (the address every name gets, the program's computer).
"""
import os, socket, struct, time

OUT = os.environ["OUT"]; ANSWER = socket.inet_aton(os.environ.get("ANSWER", "127.0.0.2"))
log = open(f"{OUT}/lookups.txt", "a")
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(("127.0.0.1", 53))
T0 = time.time()
while True:
    q, a = s.recvfrom(2048)
    try:
        tid, flags, qd = struct.unpack(">HHH", q[:6])
        i, labels = 12, []
        while q[i]:
            labels.append(q[i + 1:i + 1 + q[i]].decode("latin-1")); i += 1 + q[i]
        qtype = struct.unpack(">H", q[i + 1:i + 3])[0]
        question = q[12:i + 5]
        name = ".".join(labels).lower()
        log.write(f"{time.time()-T0:8.2f} {name} {dict([(1,'A'),(28,'AAAA'),(65,'HTTPS')]).get(qtype, qtype)}\n"); log.flush()
        if qtype == 1:
            ans = b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 5, 4) + ANSWER
            s.sendto(struct.pack(">HHHHHH", tid, 0x8180, 1, 1, 0, 0) + question + ans, a)
        else:
            s.sendto(struct.pack(">HHHHHH", tid, 0x8180, 1, 0, 0, 0) + question, a)
    except Exception as e:
        log.write(f"bad query: {e!r}\n"); log.flush()
