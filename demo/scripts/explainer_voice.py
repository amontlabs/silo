"""Record the explainer's narration with a local voice model, and mix the track.

    explainer_voice.py speak <jobs.json> <clips dir>   record each missing sentence
    explainer_voice.py mix <placements.json> <out.wav> lay the clips on one track

Runs in the virtual environment that scripts/explainer.mjs creates. Nothing is
sent anywhere: the model (Kokoro, Apache-2.0) is downloaded once and runs here.
"""

import json
import sys
from pathlib import Path

import numpy as np
import soundfile as sf

MODEL = "mlx-community/Kokoro-82M-bf16"
RATE = 24_000


def trim(audio: np.ndarray) -> np.ndarray:
    """Drop the silence the model leaves around a sentence, keeping a short tail."""
    loud = np.flatnonzero(np.abs(audio) > 0.004)
    if loud.size == 0:
        return audio
    start = max(0, loud[0] - int(0.02 * RATE))
    end = min(audio.size, loud[-1] + int(0.08 * RATE))
    return audio[start:end]


def speak(jobs_path: Path, clips: Path) -> None:
    jobs = json.loads(jobs_path.read_text())
    clips.mkdir(parents=True, exist_ok=True)
    pending = [job for job in jobs["sentences"] if not (clips / job["file"]).exists()]
    if pending:
        from mlx_audio.tts.utils import load_model

        model = load_model(MODEL)
        for number, job in enumerate(pending, 1):
            parts = [
                np.asarray(result.audio, dtype=np.float32).reshape(-1)
                for result in model.generate(
                    text=job["spoken"], voice=jobs["voice"], speed=jobs["speed"], lang_code=jobs["voice"][0]
                )
            ]
            audio = trim(np.concatenate(parts))
            sf.write(clips / job["file"], audio, RATE, subtype="PCM_16")
            print(f"[{number}/{len(pending)}] {job['file']} {audio.size / RATE:.2f}s", flush=True)
    seconds = {}
    for job in jobs["sentences"]:
        info = sf.info(clips / job["file"])
        seconds[job["file"]] = info.frames / info.samplerate
    (clips / "seconds.json").write_text(json.dumps(seconds))


def mix(placements_path: Path, out: Path) -> None:
    plan = json.loads(placements_path.read_text())
    track = np.zeros(int(plan["seconds"] * RATE) + RATE, dtype=np.float32)
    for clip in plan["clips"]:
        audio, rate = sf.read(clip["path"], dtype="float32")
        assert rate == RATE, clip["path"]
        start = int(clip["start"] * RATE)
        track[start : start + audio.size] += audio
    peak = float(np.max(np.abs(track))) or 1.0
    sf.write(out, track * min(1.0, 0.9 / peak), RATE, subtype="PCM_16")
    print(f"{out} {track.size / RATE:.1f}s")


if __name__ == "__main__":
    command, first, second = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    {"speak": speak, "mix": mix}[command](first, second)
