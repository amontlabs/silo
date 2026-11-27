"""#371's night with what the morning screen needs and #371 does not record.

    python3 patch_night.py <out folder>

Starts from #399's copy of #371's prototype with its boundary fix (`patch_proto.py --fix`: custom
tool calls read, the program's session kept on each model call), then:

- The boundary ends TLS for the hosts in the night's `ends_tls` with a certificate of the night's
  own authority, as #378's boundary does for the hosts it reads. A handshake the client refuses is
  a fact, `tls refused`, with the host and the client's alert (#403: only the side that ends TLS
  sees it; the proxy logs the connection as allowed).
- A fifth run: Codex in main, asked to update the README, with an API key placeholder and the
  proxy set but not Silo's authority. It is the case #403 found silent: Codex refuses the
  certificate, falls back, and waits for the network for ever. The night does not wait for it.

Every change is asserted to apply once.
"""
import os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PATCH_399 = os.path.join(HERE, "..", "2026-11-25-attribution-without-processes", "patch_proto.py")
out = os.path.abspath(sys.argv[1])
subprocess.run([sys.executable, PATCH_399, out, "--fix"], check=True, stdout=subprocess.DEVNULL)


def edit(name, old, new):
    p = os.path.join(out, name)
    s = open(p).read()
    assert s.count(old) == 1, (name, old[:70])
    open(p, "w").write(s.replace(old, new))


# ---- the boundary ends TLS for some hosts and records each refused handshake
edit("engine.py", '''            if REAL_NET:
                return self.tunnel(host, port, pid, t0)''', '''            if host in CFG.get("ends_tls", {}):
                return self.end_tls(host, port, pid, t0)
            if REAL_NET:
                return self.tunnel(host, port, pid, t0)''')
edit("engine.py", '''        def refuse(self, msg, code=403):''', '''        def end_tls(self, host, port, pid, t0):
            """TLS ended here with the night's authority. The prototype has no upstream for these
            hosts: a handshake that succeeds is answered 502. One that fails is the fact."""
            f = c.w.fact("connection", "boundary", host=host, port=port, decision="allowed", decided_by=None, bytes_out=0, bytes_in=0, opened=True, note="TLS ended by the boundary")
            if pid: c.w.report("owner", "probe", pid=pid, about=f["id"])
            new_host(c, host, f)
            self.wfile.write(b"HTTP/1.1 200 Connection established\\r\\n\\r\\n"); self.wfile.flush()
            ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            ctx.load_cert_chain(*CFG["ends_tls"][host])
            self.close_connection = True
            try:
                s = ctx.wrap_socket(self.connection, server_side=True)
            except (ssl.SSLError, OSError) as e:
                reason = getattr(e, "reason", None) or ""
                alert = reason[len("TLSV1_ALERT_"):].lower() if reason.startswith("TLSV1_ALERT_") else ("closed by the client" if isinstance(e, (ssl.SSLEOFError, ConnectionError)) or "EOF" in str(e) else (reason or type(e).__name__).lower())
                g = c.w.fact("tls refused", "boundary", host=host, alert=alert, connection=f["id"])
                if pid: c.w.report("owner", "probe", pid=pid, about=g["id"])
                return
            try:
                s.sendall(b"HTTP/1.1 502 Bad Gateway\\r\\ncontent-length: 0\\r\\nconnection: close\\r\\n\\r\\n")
                s.close()
            except OSError:
                pass

        def refuse(self, msg, code=403):''')

# ---- the night: its own authority, and Codex in main without it
edit("night.py", '''    rules = [dict(r, after_s=STUCK_AFTER)''', '''    tls = make_authority(f"{NIGHT}/authority", ["api.openai.com"])
    rules = [dict(r, after_s=STUCK_AFTER)''')
edit("night.py", '''           "internet": {"bun.sh": {"/install": f"{NIGHT}/internet/bun.sh/install"}},''',
     '''           "internet": {"bun.sh": {"/install": f"{NIGHT}/internet/bun.sh/install"}}, "ends_tls": tls,''')
edit("night.py", '''    if REAL:
        time.sleep(5)
        runs.append(launch("main", main_home, app, claude_argv(ASK_5)''', '''    time.sleep(3)
    # the fifth run: Silo's authority is not where Codex looks (no SSL_CERT_FILE, not in the store)
    os.makedirs(f"{main_home}/.codex", exist_ok=True)
    no_ca = launch("main", main_home, app, [CODEX, "exec", "--json", "--dangerously-bypass-approvals-and-sandbox", "--skip-git-repo-check", "-m", "gpt-5.5", ASK_6], "main-launch-5",
                   {"CODEX_HOME": f"{main_home}/.codex", "OPENAI_API_KEY": "placeholder-not-a-key", "RUST_LOG": "warn"}, PORTS["main"])
    if REAL:
        time.sleep(5)
        runs.append(launch("main", main_home, app, claude_argv(ASK_5)''')
edit("night.py", '''    time.sleep(16)  # one more interval checkpoint''', '''    runs.append(no_ca)
    time.sleep(16)  # one more interval checkpoint''')
edit("night.py", '''ASK_5 = ''', '''ASK_6 = "Update the README's setup section for the file-backed sessions."
ASK_5 = ''')
edit("night.py", '''def launch(computer,''', '''def make_authority(folder, hosts):
    """The night's own certificate authority and one certificate per host, signed by it."""
    os.makedirs(folder, exist_ok=True)
    ca, cakey = f"{folder}/ca.pem", f"{folder}/ca.key"
    q = dict(check=True, capture_output=True)
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-subj", "/CN=Silo night authority", "-keyout", cakey, "-out", ca], **q)
    out = {}
    for h in hosts:
        key, csr, crt, ext = (f"{folder}/{h}.{x}" for x in ("key", "csr", "pem", "ext"))
        open(ext, "w").write(f"subjectAltName=DNS:{h}\\n")
        subprocess.run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-subj", f"/CN={h}", "-keyout", key, "-out", csr], **q)
        subprocess.run(["openssl", "x509", "-req", "-in", csr, "-CA", ca, "-CAkey", cakey, "-CAcreateserial", "-days", "2", "-extfile", ext, "-out", crt], **q)
        out[h] = [crt, key]
    return out


def launch(computer,''')
print(out)
