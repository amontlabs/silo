"""A stand-in for Silo's boundary, for SSH only. Throwaway research code.

Copied from research/2026-11-25-credential-brokering/ssh/silo_ssh.py, with three additions:

- The relay times each connection's key exchange ("kex_gap_ms"): from the moment it hands the
  server's KEX_ECDH_REPLY to the computer, to the first encrypted packet of the client. That packet
  is encrypted with keys derived from the server's ephemeral key in that reply, so no client can
  send it sooner than one round trip to wherever the session keys are made.
- A deadline after each signature (config "deadline_ms"). The signed login request has to reach
  whoever holds the session keys and come back encrypted, so it cannot arrive sooner than one round
  trip to them. After the agent signs for a connection, a client packet that arrives later than the
  deadline, before the server has sent anything since the signature, closes the connection without
  being forwarded. The signature is for that connection's session id only, so it dies with it.
- A host-key check the boundary answers (config "hostcheck_socket"), for KnownHostsCommand, and the
  agent then signs only for a connection whose host key the computer asked it to check.

Every chunk each way is kept, with its time and size, in "trace" (JSON lines).

Two parts in one process, because in Silo they are one boundary:

1. A relay for the computer's TCP connections to port 22. It forwards every byte unchanged, and
   reads the server's side in the clear until the first NEWKEYS: the host key and the host's
   signature of the exchange hash, from KEX_ECDH_REPLY (or KEXDH_REPLY, KEX_DH_GEX_REPLY). It
   records each (host key, signature) with the name the computer connected to, for as long as the
   connection is open.

2. An SSH agent (OpenSSH's agent protocol) that holds the real private keys. The computer reaches
   it through a socket (vsock in Silo, a Unix socket here). Policy, checked in this order:
   - a sign request on an agent connection with no session-bind is refused, except SSHSIG in the
     namespaces the key allows (default "git", for commit signing);
   - session-bind@openssh.com is accepted only if its host signature is one the relay saw on a
     connection that is open now, the signature verifies over the session id with that host key,
     the host key is the one the boundary's own known_hosts holds for that name, and the key's
     list of hosts holds that name;
   - a userauth sign request must carry the bound session id, and, for the hostbound method, the
     bound host key;
   - forwarding binds (is_forwarding) are refused: the next hop is outside Silo's sight.
   Every request is a fact in the record (JSON lines), with no private value in it.

Mode "silo+infer" also signs a userauth request that came with no session-bind, when the boundary
finds an open connection whose host signature verifies over the session id in the request.

Mode "plain" turns all policy off: it is the agent forwarding Docker Sandboxes does by default, to
compare.
"""
import asyncio, base64, hashlib, json, os, socket, struct, sys, time
from cryptography.hazmat.primitives.asymmetric import ed25519, ec, rsa, padding, utils
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.exceptions import InvalidSignature

CFG = json.load(open(sys.argv[1]))
MODE = CFG.get("mode", "silo")  # "silo", "silo+infer" or "plain"
RECORD = open(CFG["record"], "a")
SEEN = {}  # host signature blob -> {"name", "hostkey", "open", "conn"}
DEADLINE_MS = CFG.get("deadline_ms")  # None: measure only
TRACE = open(CFG["trace"], "a") if CFG.get("trace") else None
now = lambda: time.perf_counter() * 1000  # ms, monotonic


def fact(**kw):
    kw["t"] = round(time.time(), 4)
    RECORD.write(json.dumps(kw) + "\n"); RECORD.flush()


# wire helpers ---------------------------------------------------------------------------------

def sstr(b): return struct.pack(">I", len(b)) + b


class Reader:
    def __init__(self, b): self.b, self.i = b, 0
    def u8(self): v = self.b[self.i]; self.i += 1; return v
    def u32(self): v = struct.unpack(">I", self.b[self.i:self.i + 4])[0]; self.i += 4; return v
    def s(self):
        n = self.u32(); v = self.b[self.i:self.i + n]
        if len(v) != n: raise ValueError("short string")
        self.i += n; return v
    def done(self): return self.i == len(self.b)


def fp(blob): return "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")


# host signature check ---------------------------------------------------------------------------

def verify_host_sig(hostkey, sig, data):
    k, s = Reader(hostkey), Reader(sig)
    kt, st = k.s().decode(), s.s().decode()
    raw = s.s()
    try:
        if kt == "ssh-ed25519" and st == "ssh-ed25519":
            ed25519.Ed25519PublicKey.from_public_bytes(k.s()).verify(raw, data); return True
        if kt.startswith("ecdsa-sha2-nistp") and st == kt:
            curve = {"nistp256": (ec.SECP256R1(), hashes.SHA256()), "nistp384": (ec.SECP384R1(), hashes.SHA384()),
                     "nistp521": (ec.SECP521R1(), hashes.SHA512())}[kt.split("-")[-1]]
            k.s(); q = k.s()
            pub = ec.EllipticCurvePublicKey.from_encoded_point(curve[0], q)
            rs = Reader(raw); r = int.from_bytes(rs.s(), "big"); ss = int.from_bytes(rs.s(), "big")
            pub.verify(utils.encode_dss_signature(r, ss), data, ec.ECDSA(curve[1])); return True
        if kt == "ssh-rsa" and st in ("rsa-sha2-256", "rsa-sha2-512"):
            e = int.from_bytes(k.s(), "big"); n = int.from_bytes(k.s(), "big")
            pub = rsa.RSAPublicNumbers(e, n).public_key()
            pub.verify(raw, data, padding.PKCS1v15(), hashes.SHA256() if st.endswith("256") else hashes.SHA512()); return True
    except InvalidSignature:
        return False
    return False


# 1. the relay ---------------------------------------------------------------------------------

async def pipe(r, w):
    try:
        while (b := await r.read(65536)):
            w.write(b); await w.drain()
    except (ConnectionError, asyncio.CancelledError):
        pass
    finally:
        try: w.close()
        except Exception: pass


async def sniff_client_side(r, w, c):
    """Forward computer -> server unchanged. Read the client's packets in the clear until its
    NEWKEYS; the first byte after it starts the first encrypted packet."""
    buf, version_done, state = b"", False, "clear"
    try:
        while (b := await r.read(65536)):
            t = now()
            if c.get("deadline") is not None and t > c["deadline"]:
                late = round(t - c["signed_at"], 3)
                fact(kind="ssh.cut", host=c["name"], conn=c["id"], after_signature_ms=late, deadline_ms=DEADLINE_MS,
                     why="the client answered the signature later than the deadline: the session keys are far away")
                c["cut"] = late
                break
            w.write(b); await w.drain()
            c["trace"].append((round(t - c["t0"], 3), "c>s", len(b)))
            if state == "done": continue
            buf += b
            if not version_done:
                if b"\n" not in buf: continue
                _, _, buf = buf.partition(b"\n"); version_done = True
            if state == "clear":
                while len(buf) >= 5:
                    plen = struct.unpack(">I", buf[:4])[0]
                    if len(buf) < 4 + plen: break
                    typ = buf[5]; buf = buf[4 + plen:]
                    if typ == 21:
                        state = "after-newkeys"; c["t_client_newkeys"] = t; break
            if state == "after-newkeys" and buf:
                c["t_first_encrypted"] = t; state = "done"
                if c.get("t_reply") is not None:
                    c["kex_gap_ms"] = round(t - c["t_reply"], 3)
                    fact(kind="ssh.kex_timed", host=c["name"], conn=c["id"], kex_gap_ms=c["kex_gap_ms"])
    except (ConnectionError, asyncio.CancelledError):
        pass
    finally:
        try: w.close()
        except Exception: pass


async def sniff_server_side(r, w, name, seen_here, c):
    """Forward server -> computer unchanged, reading packets until the server's NEWKEYS."""
    buf, version_done, sniffing = b"", False, True
    try:
        while (b := await r.read(65536)):
            w.write(b); await w.drain()
            t = now()
            c["deadline"] = None  # the server spoke: what the client sends next answers it
            c["trace"].append((round(t - c["t0"], 3), "s>c", len(b)))
            if not sniffing: continue
            buf += b
            if not version_done:
                if b"\r\n" not in buf and b"\n" not in buf: continue
                line, _, buf = buf.partition(b"\n")
                while not line.startswith(b"SSH-"):  # banner lines before the version
                    if b"\n" not in buf: break
                    line, _, buf = buf.partition(b"\n")
                version_done = True
            while len(buf) >= 5:
                plen = struct.unpack(">I", buf[:4])[0]
                if len(buf) < 4 + plen: break
                pad = buf[4]; payload = buf[5:4 + plen - pad]; buf = buf[4 + plen:]
                typ = payload[0]
                if typ in (31, 33):  # KEX_ECDH_REPLY / KEXDH_REPLY / KEX_DH_GEX_REPLY
                    rd = Reader(payload[1:]); hostkey = rd.s(); rd.s(); sig = rd.s()
                    SEEN[sig] = {"name": name, "hostkey": hostkey, "open": True, "conn": c}
                    c["t_reply"] = t
                    seen_here.append(sig)
                    fact(kind="ssh.kex_seen", host=name, hostkey=fp(hostkey))
                if typ == 21:  # NEWKEYS: the rest is encrypted
                    sniffing = False; break
    except (ConnectionError, asyncio.CancelledError):
        pass
    finally:
        for s in seen_here:
            if s in SEEN: SEEN[s]["open"] = False
        try: w.close()
        except Exception: pass


def relay_handler(name, up_host, up_port):
    async def handle(cr, cw):
        c = {"id": os.urandom(3).hex(), "name": name, "t0": now(), "trace": []}
        fact(kind="ssh.connect", host=name, conn=c["id"])
        try:
            ur, uw = await asyncio.open_connection(up_host, up_port)
        except OSError as e:
            fact(kind="ssh.connect_failed", host=name, error=str(e)); cw.close(); return
        seen_here = []
        await asyncio.gather(sniff_client_side(cr, uw, c), sniff_server_side(ur, cw, name, seen_here, c))
        fact(kind="ssh.closed", host=name, conn=c["id"])
        if TRACE:
            TRACE.write(json.dumps({k: v for k, v in c.items() if k != "t0"}) + "\n"); TRACE.flush()
    return handle


# 2. the agent ---------------------------------------------------------------------------------

KEYS = []  # {"blob", "priv", "comment", "hosts", "sshsig_namespaces"}
for k in CFG["keys"]:
    priv = serialization.load_ssh_private_key(open(k["private_key_file"], "rb").read(), None)
    blob = priv.public_key().public_bytes(serialization.Encoding.OpenSSH, serialization.PublicFormat.OpenSSH)
    blob = base64.b64decode(blob.split()[1])
    KEYS.append({"blob": blob, "priv": priv, "comment": k.get("comment", ""), "hosts": k["hosts"],
                 "ns": k.get("sshsig_namespaces", ["git"])})

KNOWN = {}  # name -> set of host key blobs (the boundary's own known_hosts, never the computer's)
for line in open(CFG["known_hosts"]):
    parts = line.split()
    if len(parts) >= 3: KNOWN.setdefault(parts[0], set()).add(base64.b64decode(parts[2]))


def sign(key, data, flags):
    p = key["priv"]
    if isinstance(p, ed25519.Ed25519PrivateKey):
        return sstr(b"ssh-ed25519") + sstr(p.sign(data))
    raise ValueError("only ed25519 keys in this prototype")


def safe(f, data):
    try: return f(data)
    except (ValueError, IndexError, struct.error): return None


def parse_userauth(data):
    """RFC 4252 publickey signature data, and OpenSSH's publickey-hostbound-v00 variant."""
    rd = Reader(data)
    sid = rd.s()
    if rd.u8() != 50: return None
    user = rd.s(); service = rd.s(); method = rd.s()
    if method not in (b"publickey", b"publickey-hostbound-v00@openssh.com"): return None
    if rd.u8() != 1: return None
    alg = rd.s(); keyblob = rd.s()
    hostkey = rd.s() if method.endswith(b"openssh.com") else None
    if not rd.done(): return None
    return {"sid": sid, "user": user.decode(errors="replace"), "service": service, "method": method.decode(),
            "keyblob": keyblob, "hostkey": hostkey}


def parse_sshsig(data):
    if not data.startswith(b"SSHSIG"): return None
    rd = Reader(data[6:])
    ns = rd.s(); rd.s(); halg = rd.s(); h = rd.s()
    return {"namespace": ns.decode(errors="replace"), "hash": halg.decode()} if rd.done() else None


async def agent_conn(r, w):
    conn = {"binds": [], "id": os.urandom(3).hex()}
    try:
        while True:
            hdr = await r.readexactly(4)
            msg = await r.readexactly(struct.unpack(">I", hdr)[0])
            reply = handle_agent(msg, conn)
            w.write(sstr(reply)); await w.drain()
    except (asyncio.IncompleteReadError, ConnectionError):
        pass
    finally:
        w.close()


FAIL, OK = bytes([5]), bytes([6])


def check_hostkey(name, hostkey):
    return None if hostkey in KNOWN.get(name, set()) else "host key not in the boundary's known_hosts for this name"


def infer_bind(sid):
    for sig, seen in SEEN.items():
        if seen["open"] and verify_host_sig(seen["hostkey"], sig, sid):
            fact(kind="ssh.agent_bound", how="inferred", host=seen["name"], hostkey=fp(seen["hostkey"]))
            return {"sid": sid, "hostkey": seen["hostkey"], "name": seen["name"], "kc": seen["conn"]}
    return None


def refuse(why, **kw):
    fact(kind="ssh.agent_refused", why=why, **kw); return FAIL


def handle_agent(msg, conn):
    t, rd = msg[0], Reader(msg[1:])
    if t == 11:  # REQUEST_IDENTITIES
        fact(kind="ssh.agent_list", conn=conn["id"])
        out = struct.pack(">I", len(KEYS))
        for k in KEYS: out += sstr(k["blob"]) + sstr(k["comment"].encode())
        return bytes([12]) + out
    if t == 27:  # EXTENSION
        name = rd.s()
        if name != b"session-bind@openssh.com":
            return refuse("unknown extension", ext=name.decode(errors="replace"))
        hostkey, sid, sig = rd.s(), rd.s(), rd.s(); fwd = rd.u8()
        if MODE == "plain":
            conn["binds"].append({"sid": sid, "hostkey": hostkey, "name": None}); return OK
        seen = SEEN.get(sig)
        if not seen or not seen["open"]:
            return refuse("bind: host signature never seen on an open connection of this computer",
                          conn=conn["id"], hostkey=fp(hostkey), seen_closed=bool(seen))
        if seen["hostkey"] != hostkey:
            return refuse("bind: host key differs from the one seen", conn=conn["id"])
        if not verify_host_sig(hostkey, sig, sid):
            return refuse("bind: host signature does not verify over the session id", conn=conn["id"])
        if hostkey_refusal := check_hostkey(seen["name"], hostkey):
            return refuse("bind: " + hostkey_refusal, conn=conn["id"], host=seen["name"], hostkey=fp(hostkey))
        if fwd:
            return refuse("bind: forwarding onward from the computer", conn=conn["id"], host=seen["name"])
        if any(b["sid"] == sid for b in conn["binds"]):
            return refuse("bind: duplicate session id", conn=conn["id"])
        conn["binds"].append({"sid": sid, "hostkey": hostkey, "name": seen["name"], "kc": seen["conn"]})
        fact(kind="ssh.agent_bound", conn=conn["id"], host=seen["name"], hostkey=fp(hostkey))
        return OK
    if t == 13:  # SIGN_REQUEST
        keyblob, data, flags = rd.s(), rd.s(), rd.u32()
        key = next((k for k in KEYS if k["blob"] == keyblob), None)
        if not key: return refuse("sign: unknown key")
        ua, ss = safe(parse_userauth, data), safe(parse_sshsig, data)
        if MODE == "plain":
            fact(kind="ssh.agent_signed", mode="plain", what="userauth" if ua else "sshsig" if ss else "other")
            return bytes([14]) + sstr(sign(key, data, flags))
        if ss:
            if ss["namespace"] not in key["ns"]:
                return refuse("sshsig: namespace not allowed for this key", namespace=ss["namespace"])
            fact(kind="ssh.agent_signed", what="sshsig", namespace=ss["namespace"], key=fp(keyblob))
            return bytes([14]) + sstr(sign(key, data, flags))
        if not ua:
            return refuse("sign: data is neither a userauth request nor SSHSIG", conn=conn["id"], size=len(data))
        if conn["binds"]:
            bind = conn["binds"][-1]
        elif MODE == "silo+infer":
            # No session-bind (Paramiko, asyncssh, Go, OpenSSH before 8.9). The session id is in the
            # data to sign, and the host signed it: find an open connection whose host signature
            # verifies over it. That is the bind, made by the boundary instead of the client.
            bind = infer_bind(ua["sid"])
            if bind is None:
                return refuse("sign: no open connection of this computer has this session id", conn=conn["id"],
                              user=ua["user"])
            if hostkey_refusal := check_hostkey(bind["name"], bind["hostkey"]):
                return refuse(hostkey_refusal, conn=conn["id"], host=bind["name"])
        else:
            return refuse("sign: userauth on an agent connection with no session-bind", conn=conn["id"], user=ua["user"])
        if ua["sid"] != bind["sid"]:
            return refuse("sign: session id differs from the bound one", conn=conn["id"])
        if ua["hostkey"] is not None and ua["hostkey"] != bind["hostkey"]:
            return refuse("sign: hostbound host key differs from the bound one", conn=conn["id"])
        if ua["keyblob"] != keyblob:
            return refuse("sign: key in the request differs from the key asked", conn=conn["id"])
        if bind["name"] not in key["hosts"]:
            return refuse("sign: host not on this key's list", conn=conn["id"], host=bind["name"])
        kc = bind.get("kc") or {}
        if CFG.get("hostcheck_socket") and not any(h == bind["hostkey"] and t >= kc.get("t0", 0) for t, h in CHECKED):
            return refuse("sign: the computer never asked the boundary to check this connection's host key",
                          conn=conn["id"], host=bind["name"])
        gap = kc.get("kex_gap_ms")
        out = bytes([14]) + sstr(sign(key, data, flags))
        if kc:
            kc["signed_at"] = now()
            kc.setdefault("agent", []).append((round(kc["signed_at"] - kc["t0"], 3), "signed"))
            if DEADLINE_MS is not None: kc["deadline"] = kc["signed_at"] + DEADLINE_MS
        fact(kind="ssh.agent_signed", what="userauth", host=bind["name"], user=ua["user"],
             method=ua["method"], key=fp(keyblob), conn=conn["id"], tcp=kc.get("id"), kex_gap_ms=gap)
        return out
    return refuse("unsupported agent message", type=t)


CHECKED = []  # (time, host key blob) the computer asked the boundary to check


async def hostcheck_conn(r, w):
    """KnownHostsCommand's other end: '<host> <type> <key base64>' in, a known_hosts line out if
    the boundary knows that key."""
    try:
        host, typ, k = (await r.readline()).decode().split()[:3]
        blob = base64.b64decode(k)
        if any(blob in keys for keys in KNOWN.values()):
            CHECKED.append((now(), blob)); fact(kind="ssh.hostkey_checked", hostkey=fp(blob))
            w.write(f"{host} {typ} {k}\n".encode()); await w.drain()
    except (ValueError, ConnectionError):
        pass
    w.close()


async def main():
    servers = []
    if CFG.get("hostcheck_socket"):
        if os.path.exists(CFG["hostcheck_socket"]): os.unlink(CFG["hostcheck_socket"])
        servers.append(await asyncio.start_unix_server(hostcheck_conn, CFG["hostcheck_socket"]))
    for r in CFG["relays"]:
        servers.append(await asyncio.start_server(relay_handler(r["name"], r["upstream_host"], r["upstream_port"]),
                                                  r.get("listen_host", "127.0.0.1"), r["listen_port"]))
    if os.path.exists(CFG["agent_socket"]): os.unlink(CFG["agent_socket"])
    servers.append(await asyncio.start_unix_server(agent_conn, CFG["agent_socket"]))
    fact(kind="boundary.start", mode=MODE)
    print("ready", flush=True)
    await asyncio.gather(*(s.serve_forever() for s in servers))


asyncio.run(main())
