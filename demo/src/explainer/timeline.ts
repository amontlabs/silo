import { SCENES, type SceneNarration } from "./narration.ts";
import { TIMING } from "./timing.generated.ts";

export const FPS = 30;
export const WIDTH = 1920;
export const HEIGHT = 1080;

/** Silence before a scene's first sentence, so its picture settles first. */
const LEAD_IN = 0.35;
/** Silence after each sentence, and after a scene's last one. */
const GAP = 0.26;
const SCENE_GAP = 0.6;

export type TimedBeat = {
  key: string;
  /** Caption text, as written. */
  caption: string;
  /** Text handed to the voice, with pronunciations substituted. */
  spoken: string;
  /** Scene-local frame at which the sentence starts. */
  from: number;
  /** Frames until the next sentence starts (or the scene ends). */
  frames: number;
  /** Seconds of recorded audio. */
  audio: number;
  file: string;
};

export type TimedScene = {
  id: string;
  chapter: number;
  title: string;
  from: number;
  frames: number;
  beats: TimedBeat[];
};

/** `{caption|spoken}` writes one thing and says another; plain text does both. */
const MARK = /\{([^|{}]*)\|([^|{}]*)\}/g;
export const captionOf = (text: string) => text.replace(MARK, "$1");
export const spokenOf = (text: string) => text.replace(MARK, "$2");

/** Speaking time when a sentence has no recording yet. */
const estimate = (spoken: string) => Math.max(1, spoken.split(/\s+/).length / 2.7);

export const beatFile = (sceneId: string, key: string) => `${sceneId}--${key}.wav`;

const timeScene = (scene: SceneNarration, from: number): TimedScene => {
  let cursor = Math.round(LEAD_IN * FPS);
  const beats = scene.beats.map(([key, text, options], index): TimedBeat => {
    const spoken = spokenOf(text);
    const file = beatFile(scene.id, key);
    const recorded = TIMING[file];
    const audio = recorded && recorded.spoken === spoken ? recorded.seconds : estimate(spoken);
    const last = index === scene.beats.length - 1;
    const gap = (last ? SCENE_GAP : GAP) + (options?.pause ?? 0);
    const frames = Math.ceil((audio + gap) * FPS);
    const beat = { key, caption: captionOf(text), spoken, from: cursor, frames, audio, file };
    cursor += frames;
    return beat;
  });
  return { id: scene.id, chapter: scene.chapter, title: scene.title, from, frames: cursor, beats };
};

export const TIMELINE: TimedScene[] = (() => {
  const timed: TimedScene[] = [];
  let from = 0;
  for (const scene of SCENES) {
    const next = timeScene(scene, from);
    timed.push(next);
    from += next.frames;
  }
  return timed;
})();

export const DURATION = TIMELINE.reduce((total, scene) => total + scene.frames, 0);
