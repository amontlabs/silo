"""listen.py <film dir>: transcribes every clip of voice.json with faster-whisper (offline) and lists the words
that differ from the script. Exit 0: every clip matches; 1: some differ; 3: skipped (faster_whisper or its model missing)."""
import _venv  # noqa: F401  (W/.venv, offline)
import difflib, json, os, pathlib, re, sys
import place
MODEL = "mobiuslabsgmbh/faster-whisper-large-v3-turbo"
NUM = "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty".split()
try:
    from faster_whisper import WhisperModel
except Exception as e:
    print(f"skipped: faster_whisper does not import ({e.__class__.__name__})")
    sys.exit(3)


def words(s):
    s = s.lower().replace("’", "'")
    s = re.sub(r"\b(\d+)\b", lambda m: NUM[int(m.group(1))] if int(m.group(1)) < len(NUM) else m.group(1), s)
    return re.findall(r"[a-z0-9']+", s)


ch = place.film(sys.argv[1])
voice = json.loads((ch / "voice.json").read_text())
try:
    model = WhisperModel(MODEL, device="cpu", compute_type="int8", local_files_only=True)
except Exception as e:
    print(f"skipped: the Whisper model {MODEL} in the Hugging Face cache is incomplete (its weights and tokenizer"
          f" are missing), and this check downloads nothing. ({e.__class__.__name__})")
    sys.exit(3)
differ = 0
for v in voice["voices"]:
    for si, sc in enumerate(v["scenes"]):
        for j, x in enumerate(sc["sentences"]):
            segs, _ = model.transcribe(str(place.clip(ch, x["file"])), language="en", beam_size=5, vad_filter=False)
            heard = " ".join(s.text for s in segs)
            a, b = words(x["text"]), words(heard)
            if a == b:
                continue
            differ += 1
            ops = [f"said \"{' '.join(b[j1:j2])}\" for \"{' '.join(a[i1:i2])}\""
                   for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b).get_opcodes() if tag != "equal"]
            print(f"{v['id']} scene {si + 1} sentence {j + 1}: " + "; ".join(ops))
total = sum(len(sc["sentences"]) for v in voice["voices"] for sc in v["scenes"])
print(f"{total - differ} of {total} clips heard exactly as written")
sys.exit(1 if differ else 0)
