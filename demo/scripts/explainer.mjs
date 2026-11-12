// Builds the narrated explainer film (composition SiloExplainer).
//
//   node --experimental-strip-types scripts/explainer.mjs voice    record narration, write timings, mix the track
//   node --experimental-strip-types scripts/explainer.mjs listen   transcribe the narration back, list what differs
//   node --experimental-strip-types scripts/explainer.mjs stills   one frame per scene, as contact sheets
//   node --experimental-strip-types scripts/explainer.mjs render   the film, with narration
//
// Everything generated lands in out/explainer/ (ignored). `voice` reuses a
// recorded sentence until its spoken text changes.
import { execFileSync, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const demo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const out = path.join(demo, "out/explainer");
const publicDir = path.join(out, "public");
const clips = path.join(out, "clips");
const venv = path.join(out, "tts-venv");
const python = path.join(venv, "bin/python");
const entry = path.join(demo, "src/explainer/index.ts");
const VOICE = { voice: "af_heart", speed: 1.05 };

const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { stdio: "inherit", cwd: demo, ...options });
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} exited with ${result.status}`);
};

const loadTimeline = () => import(`../src/explainer/timeline.ts?${Date.now()}`);

const ensureVenv = () => {
  if (fs.existsSync(python)) return;
  run("uv", ["venv", venv, "--python", "3.12", "-q"]);
  run("uv", ["pip", "install", "-q", "--python", python, "mlx-audio==0.4.2", "misaki[en]", "soundfile"]);
};

const voice = async () => {
  ensureVenv();
  fs.mkdirSync(publicDir, { recursive: true });
  const { TIMELINE } = await loadTimeline();
  // A clip is named by what is said and how, so an edited sentence is recorded again.
  const clipName = (spoken) =>
    `${crypto.createHash("sha256").update(JSON.stringify([spoken, VOICE])).digest("hex").slice(0, 20)}.wav`;
  const beats = TIMELINE.flatMap((scene) => scene.beats.map((beat) => ({ ...beat, clip: clipName(beat.spoken) })));
  const jobs = path.join(out, "voice-jobs.json");
  const unique = [...new Map(beats.map((beat) => [beat.clip, { file: beat.clip, spoken: beat.spoken }])).values()];
  fs.writeFileSync(jobs, JSON.stringify({ ...VOICE, sentences: unique }));
  const environment = { ...process.env, VIRTUAL_ENV: venv, PATH: `${path.join(venv, "bin")}:${process.env.PATH}` };
  run(python, [path.join(demo, "scripts/explainer_voice.py"), "speak", jobs, clips], { env: environment });

  const seconds = JSON.parse(fs.readFileSync(path.join(clips, "seconds.json"), "utf8"));
  const timing = Object.fromEntries(
    beats.map((beat) => [beat.file, { seconds: Number(seconds[beat.clip].toFixed(3)), spoken: beat.spoken }]),
  );
  fs.writeFileSync(
    path.join(demo, "src/explainer/timing.generated.ts"),
    "// Written by scripts/explainer.mjs from the recorded narration. Do not edit.\n" +
      `export const TIMING: Record<string, { seconds: number; spoken: string }> = ${JSON.stringify(timing, null, 2)};\n`,
  );

  // Timings changed, so place the clips from a fresh process that reads them.
  run(process.execPath, ["--experimental-strip-types", fileURLToPath(import.meta.url), "mix"]);
};

const mix = async () => {
  const { TIMELINE, DURATION, FPS } = await loadTimeline();
  const clipName = (spoken) =>
    `${crypto.createHash("sha256").update(JSON.stringify([spoken, VOICE])).digest("hex").slice(0, 20)}.wav`;
  const placements = {
    seconds: DURATION / FPS,
    clips: TIMELINE.flatMap((scene) =>
      scene.beats.map((beat) => ({ path: path.join(clips, clipName(beat.spoken)), start: (scene.from + beat.from) / FPS })),
    ),
  };
  const plan = path.join(out, "placements.json");
  fs.writeFileSync(plan, JSON.stringify(placements));
  const raw = path.join(out, "narration-raw.wav");
  run(python, [path.join(demo, "scripts/explainer_voice.py"), "mix", plan, raw]);
  // The voice model records quietly; bring the track to the usual level for speech.
  run("ffmpeg", [
    "-y", "-loglevel", "error", "-i", raw,
    "-af", "loudnorm=I=-16:TP=-1.5:LRA=11", "-ar", "48000", path.join(publicDir, "narration.wav"),
  ]);
  const minutes = Math.floor(DURATION / FPS / 60);
  console.log(`film: ${minutes}m${Math.round(DURATION / FPS - minutes * 60)}s, ${TIMELINE.length} scenes`);
};

const stills = async () => {
  const { bundle } = await import("@remotion/bundler");
  const { renderStill, selectComposition } = await import("@remotion/renderer");
  const { TIMELINE } = await loadTimeline();
  // EXPLAINER_AT=<frames> looks that far into each scene; EXPLAINER_LIT=1 keeps the spotlight.
  const only = process.argv.slice(3);
  const scenes = only.length ? TIMELINE.filter((scene) => only.some((id) => scene.id.startsWith(id))) : TIMELINE;
  const dir = path.join(out, "stills");
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const serveUrl = await bundle({ entryPoint: entry, publicDir });
  const inputProps = { narration: false, flat: !process.env.EXPLAINER_LIT };
  const composition = await selectComposition({ serveUrl, id: "SiloExplainer", inputProps });
  let number = 0;
  for (const scene of scenes) {
    number += 1;
    // The frame just before the scene fades, when everything it shows has arrived.
    const frame = process.env.EXPLAINER_AT ? scene.from + Number(process.env.EXPLAINER_AT) : scene.from + scene.frames - 12;
    const output = path.join(dir, `${String(number).padStart(3, "0")}-${scene.id}.jpg`);
    await renderStill({ composition, serveUrl, frame, output, inputProps, imageFormat: "jpeg", jpegQuality: 82, scale: 0.5 });
  }
  const sheets = Math.ceil(scenes.length / 6);
  for (let sheet = 0; sheet < sheets; sheet += 1) {
    execFileSync("ffmpeg", [
      "-y", "-loglevel", "error", "-start_number", String(sheet * 6 + 1), "-pattern_type", "glob",
      "-i", path.join(dir, `{${Array.from({ length: 6 }, (_, i) => String(sheet * 6 + i + 1).padStart(3, "0")).join(",")}}-*.jpg`),
      "-vf", "tile=2x3:padding=8:color=0x333333", "-frames:v", "1", path.join(dir, `sheet-${String(sheet + 1).padStart(2, "0")}.jpg`),
    ]);
  }
  console.log(`${scenes.length} stills, ${sheets} sheets in ${dir}`);
};

const render = () => {
  const output = path.join(out, "silo-explainer.mp4");
  run(path.join(demo, "node_modules/.bin/remotion"), [
    "render", entry, "SiloExplainer", output,
    `--public-dir=${publicDir}`, "--codec=h264", "--crf=20", "--concurrency=6", "--log=error",
  ]);
  console.log(output);
};

const listen = () => {
  const environment = { ...process.env, VIRTUAL_ENV: venv, PATH: `${path.join(venv, "bin")}:${process.env.PATH}` };
  run(
    python,
    [path.join(demo, "scripts/explainer_listen.py"), path.join(out, "voice-jobs.json"), clips, path.join(out, "listen.json")],
    { env: environment },
  );
};

await { voice, mix, listen, stills, render }[process.argv[2]]();
