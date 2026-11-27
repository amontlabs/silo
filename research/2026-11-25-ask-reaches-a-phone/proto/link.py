"""The link between Silo on the Mac and Silo's phone app, over a relay neither trusts.

Pairing (done once, by a QR code on the Mac's screen in a real app) gives each side the other's
two public keys: an Ed25519 key that signs and an X25519 key that receives sealed messages, plus
two random topic names on the relay. Everything that crosses the relay is sealed to the receiver
and signed by the sender, so the relay sees a topic, a size and a time, and nothing else, and it
cannot make a yes.

    seal(obj, sender_sign_key, receiver_box_pub) -> bytes
    open_(blob, receiver_box_key, sender_sign_pub) -> obj      raises Refused
"""
import base64, json, os, time
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey, X25519PublicKey
from cryptography.hazmat.primitives.ciphers.aead import ChaCha20Poly1305
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.exceptions import InvalidSignature, InvalidTag

RAW = dict(encoding=serialization.Encoding.Raw, format=serialization.PublicFormat.Raw)
PRIV = dict(encoding=serialization.Encoding.Raw, format=serialization.PrivateFormat.Raw,
            encryption_algorithm=serialization.NoEncryption())


class Refused(Exception):
    pass


def b64(b): return base64.b64encode(b).decode()
def unb64(s): return base64.b64decode(s)


def new_side():
    s, x = Ed25519PrivateKey.generate(), X25519PrivateKey.generate()
    return {"sign": b64(s.private_bytes(**PRIV)), "box": b64(x.private_bytes(**PRIV)),
            "sign_pub": b64(s.public_key().public_bytes(**RAW)), "box_pub": b64(x.public_key().public_bytes(**RAW))}


def pair(path):
    """Write a pairing file: both sides' keys and the two topics. In a real app each side keeps
    only its own private keys; one file keeps the prototype simple."""
    p = {"mac": new_side(), "phone": new_side(),
         "to_phone": "t" + os.urandom(16).hex(), "to_mac": "t" + os.urandom(16).hex()}
    json.dump(p, open(path, "w"), indent=1)
    return p


def _kdf(shared, eph_pub, rcv_pub):
    return HKDF(hashes.SHA256(), 32, salt=eph_pub + rcv_pub, info=b"silo-ask-link-1").derive(shared)


def seal(obj, sender_sign_b64, receiver_box_pub_b64):
    msg = json.dumps(obj, sort_keys=True, separators=(",", ":")).encode()
    sig = Ed25519PrivateKey.from_private_bytes(unb64(sender_sign_b64)).sign(msg)
    eph = X25519PrivateKey.generate()
    rcv = unb64(receiver_box_pub_b64)
    eph_pub = eph.public_key().public_bytes(**RAW)
    key = _kdf(eph.exchange(X25519PublicKey.from_public_bytes(rcv)), eph_pub, rcv)
    nonce = os.urandom(12)
    ct = ChaCha20Poly1305(key).encrypt(nonce, sig + msg, eph_pub)
    return eph_pub + nonce + ct


def open_(blob, receiver_box_b64, sender_sign_pub_b64):
    if len(blob) < 32 + 12 + 16 + 64:
        raise Refused("too short")
    eph_pub, nonce, ct = blob[:32], blob[32:44], blob[44:]
    me = X25519PrivateKey.from_private_bytes(unb64(receiver_box_b64))
    key = _kdf(me.exchange(X25519PublicKey.from_public_bytes(eph_pub)), eph_pub, me.public_key().public_bytes(**RAW))
    try:
        pt = ChaCha20Poly1305(key).decrypt(nonce, ct, eph_pub)
    except InvalidTag:
        raise Refused("not sealed to this side, or changed on the way")
    sig, msg = pt[:64], pt[64:]
    try:
        Ed25519PublicKey.from_public_bytes(unb64(sender_sign_pub_b64)).verify(sig, msg)
    except InvalidSignature:
        raise Refused("not signed by the paired side")
    return json.loads(msg)


def now():
    return time.time()
