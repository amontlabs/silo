"""#379 ssh/silo_ssh.py: the agent's message handler, fed messages a computer could send.
Properties: the handler never raises (a raise drops the agent connection and logs a traceback),
and in mode "silo" with no connection seen, it signs nothing but SSHSIG in an allowed namespace."""
import atheris, sys, os, json, tempfile, subprocess, struct
d = tempfile.mkdtemp()
from cryptography.hazmat.primitives.asymmetric import ed25519 as _ed
from cryptography.hazmat.primitives import serialization as _ser
open(f"{d}/k", "wb").write(_ed.Ed25519PrivateKey.generate().private_bytes(_ser.Encoding.PEM, _ser.PrivateFormat.OpenSSH, _ser.NoEncryption()))
open(f"{d}/kh", "w").write("")
cfg = {"mode": os.environ.get("MODE", "silo"), "record": f"{d}/rec.jsonl", "keys": [{"private_key_file": f"{d}/k", "hosts": ["github.com"]}],
       "known_hosts": f"{d}/kh", "relays": [], "agent_socket": f"{d}/a.sock"}
json.dump(cfg, open(f"{d}/cfg.json", "w"))
sys.argv = ["silo_ssh.py", f"{d}/cfg.json"]
sys.path.insert(0, d)
open(f"{d}/silo_ssh_mod.py", "w").write(open(os.environ["SRC"]).read().replace("asyncio.run(main())", ""))
with atheris.instrument_imports(include=["silo_ssh_mod"]):
    import silo_ssh_mod
g = vars(silo_ssh_mod)
KEYBLOB = g["KEYS"][0]["blob"]
sstr = g["sstr"]

def one(data):
    fdp = atheris.FuzzedDataProvider(data)
    # Half the time, aim at the real key so the policy is reached.
    if fdp.ConsumeBool():
        body = fdp.ConsumeBytes(fdp.ConsumeIntInRange(0, 600))
        msg = bytes([13]) + sstr(KEYBLOB) + sstr(body) + struct.pack(">I", 0)
    else:
        msg = fdp.ConsumeBytes(800)
    if not msg: return
    conn = {"binds": [], "id": "fz"}
    try:
        r = g["handle_agent"](msg, conn)
    except (ValueError, IndexError, struct.error):
        return  # finding 1: a short message drops the agent connection (reported once, by repro)
    if r[:1] == b"\x0e" and cfg["mode"] == "silo":
        rd = g["Reader"](msg[1:]); rd.s(); data = rd.s()
        p = g["safe"](g["parse_sshsig"], data)
        if not p or p["namespace"] != "git":
            raise AssertionError(f"signed without a bind: {data[:60]!r}")

atheris.Setup([sys.argv[0]] + os.environ.get("FZARGS", "").split(), one)
atheris.Fuzz()
