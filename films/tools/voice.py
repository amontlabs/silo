#!/usr/bin/env python3
"""voice.py <film dir> [--stand-in]: speaks narration.json with Kokoro and writes voice.json.

It runs with W/.venv/bin/python, offline (tools/_venv.py), where Kokoro's misaki is installed. Without misaki it stops, unless
--stand-in is given: then macOS `say` speaks instead (Samantha for a female voice id, Daniel for a male one),
and voice.json and the player's voice switch say so.
The film folder must be a direct child of films/ (tools/place.py); it writes only in that folder. Run it again without the flag once misaki is installed.

Each sentence becomes one small mono MP3 in <film>/voice/, named by a hash of voice, speed and text,
so a sentence that did not change is never spoken again. Its length is measured on the samples before
encoding. voice.json gives, per voice and per scene, each sentence's text, file, start within the scene
and length, and the scene's total (silence before, between and after included).
"""
import _venv  # noqa: F401  (W/.venv, offline)
import hashlib, json, os, subprocess, sys, pathlib, tempfile
import place


MODEL = "mlx-community/Kokoro-82M-bf16"
BEFORE, BETWEEN, AFTER = 0.4, 0.35, 0.6   # seconds of silence around the sentences of a scene
FFMPEG = "/opt/homebrew/bin/ffmpeg"
RATE = 24000                               # Kokoro's sample rate


STAND_IN = {"f": "Samantha", "m": "Daniel"}  # by the second letter of a Kokoro voice id (af_heart, am_michael)


def key(engine, voice, speed, text):
    return hashlib.sha256(f"{engine}|{voice}|{speed}|{text}".encode()).hexdigest()[:16]


def kokoro_ready():
    try:
        import misaki.en  # noqa: F401
        return True
    except Exception:
        return False


def speak_stand_in(voice, speed, text, wav):
    """macOS `say` at its usual pace times speed, written as 24 kHz mono samples."""
    subprocess.run(["say", "-v", STAND_IN[voice[1]], "-r", str(int(180 * speed)), "-o", str(wav),
                    "--file-format=WAVE", f"--data-format=LEI16@{RATE}", text], check=True)


def trim(audio, rate):
    """Cuts the near-silence Kokoro leaves at both ends, keeping 30 ms, so the gaps are the ones set above."""
    import numpy as np
    a = np.abs(audio)
    loud = np.where(a > 10 ** (-45 / 20))[0]
    if not len(loud):
        return audio
    pad = int(0.03 * rate)
    return audio[max(0, loud[0] - pad): min(len(audio), loud[-1] + pad)]


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    ch = place.film(sys.argv[1])
    stand_in = "--stand-in" in sys.argv
    if not kokoro_ready() and not stand_in:
        sys.exit("Kokoro cannot run: the Python package misaki is missing (pip install 'misaki[en]' needs the network).\n"
                 "Run again with --stand-in to use macOS voices until it is installed.")
    engine = "kokoro" if not stand_in else "say"
    spec = json.loads((ch / "narration.json").read_text())
    speed = float(spec.get("speed", 1.0))
    out_dir = place.inside(ch, "voice")
    out_dir.mkdir(exist_ok=True)
    model = None
    used = set()
    result = {"engine": engine, "speed": speed, "before": BEFORE, "between": BETWEEN, "after": AFTER, "voices": []}
    for v in spec["voices"]:
        scenes = []
        for si, sentences in enumerate(spec["scenes"]):
            t, items = BEFORE, []
            for text in sentences:
                h = key(engine if engine == "say" else MODEL, v["id"], speed, text)
                mp3, meta = out_dir / f"{h}.mp3", out_dir / f"{h}.json"
                used |= {mp3.name, meta.name}
                if not (mp3.exists() and meta.exists()):
                    import numpy as np, soundfile as sf
                    with tempfile.TemporaryDirectory(dir=out_dir) as tmp:
                        wav = pathlib.Path(tmp) / "s.wav"
                        if engine == "say":
                            speak_stand_in(v["id"], speed, text, wav)
                            audio = sf.read(wav, dtype="float32")[0].reshape(-1)
                        else:
                            if model is None:
                                from mlx_audio.tts.utils import load_model
                                model = load_model(MODEL)
                            audio = np.concatenate([np.array(r.audio, dtype=np.float32).reshape(-1)
                                                    for r in model.generate(text=text, voice=v["id"], speed=speed,
                                                                            lang_code=v["id"][0])])
                        audio = trim(audio, RATE)
                        sf.write(wav, audio, RATE)
                        subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-i", str(wav), "-ac", "1",
                                        "-c:a", "libmp3lame", "-b:a", "48k", str(mp3)], check=True)
                    meta.write_text(json.dumps({"engine": engine, "voice": v["id"], "speed": speed, "text": text,
                                                "length": round(len(audio) / RATE, 3)}))
                    print(f"spoke  {v['id']} {si + 1}: {text[:60]}")
                length = json.loads(meta.read_text())["length"]
                items.append({"text": text, "file": f"voice/{mp3.name}", "start": round(t, 3), "length": length})
                t += length + BETWEEN
            total = round(t - BETWEEN + AFTER, 3) if items else 0
            scenes.append({"sentences": items, "total": total})
        label = v.get("label", v["id"]) + (f" (stand-in: {STAND_IN[v['id'][1]]})" if engine == "say" else "")
        result["voices"].append({"id": v["id"], "label": label, "scenes": scenes,
                                 "total": round(sum(s["total"] for s in scenes), 3)})
    (ch / "voice.json").write_text(json.dumps(result, indent=1) + "\n")
    stale = [p for p in out_dir.iterdir() if p.name not in used]
    for p in stale:
        p.unlink()
    for v in result["voices"]:
        print(f"{v['id']}: " + "  ".join(f"{s['total']:.1f}" for s in v["scenes"]) + f"  = {v['total']:.1f} s")
    print(f"wrote {ch / 'voice.json'}" + (f", removed {len(stale)} unused clips" if stale else ""))


if __name__ == "__main__":
    main()
