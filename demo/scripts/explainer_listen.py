"""Check the recorded narration by transcribing it back.

    explainer_listen.py <voice-jobs.json> <clips dir> <report.json>

Each clip is transcribed with a local speech-to-text model and compared with the
text it was recorded from. Clips that differ most are printed first, so a
mispronounced name or a dropped word shows up without anyone listening.
"""

import difflib
import json
import re
import sys
from pathlib import Path

MODEL = "mlx-community/parakeet-tdt-0.6b-v2"


def words(text: str) -> list[str]:
    return re.findall(r"[a-z0-9]+", text.lower())


def main(jobs_path: Path, clips: Path, report_path: Path) -> None:
    from mlx_audio.stt.utils import load_model

    jobs = json.loads(jobs_path.read_text())["sentences"]
    known = json.loads(report_path.read_text()) if report_path.exists() else {}
    model = load_model(MODEL)
    report = {}
    for number, job in enumerate(jobs, 1):
        previous = known.get(job["file"])
        if previous and previous["spoken"] == job["spoken"]:
            report[job["file"]] = previous
            continue
        heard = model.generate(str(clips / job["file"])).text.strip()
        score = difflib.SequenceMatcher(None, words(job["spoken"]), words(heard)).ratio()
        report[job["file"]] = {"spoken": job["spoken"], "heard": heard, "score": round(score, 3)}
        print(f"[{number}/{len(jobs)}] {score:.2f}", flush=True)
    report_path.write_text(json.dumps(report, indent=1))
    worst = sorted(report.values(), key=lambda entry: entry["score"])
    for entry in worst[:45]:
        if entry["score"] >= 0.97:
            break
        print(f"\n{entry['score']:.2f}\n  said:  {entry['spoken']}\n  heard: {entry['heard']}")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
