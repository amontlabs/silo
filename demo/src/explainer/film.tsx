import React from "react";
import { AbsoluteFill, Audio, Sequence, interpolate, staticFile, useCurrentFrame } from "remotion";
import { INK, SANS, Stage } from "./kit.tsx";
import { CHAPTERS } from "./narration.ts";
import { DURATION, TIMELINE, type TimedScene } from "./timeline.ts";
import { VISUALS } from "./scenes/index.ts";

const Backdrop: React.FC = () => {
  const frame = useCurrentFrame();
  const drift = (frame / 9) % 44;
  return (
    <AbsoluteFill style={{ background: INK.bg }}>
      <AbsoluteFill
        style={{
          backgroundImage: "radial-gradient(circle, rgba(243,238,228,0.07) 1.4px, transparent 1.6px)",
          backgroundSize: "44px 44px",
          backgroundPosition: `${drift}px ${drift / 2}px`,
        }}
      />
      <AbsoluteFill
        style={{
          background:
            "radial-gradient(1200px 700px at 78% 8%, rgba(255,159,10,0.07), transparent 70%), radial-gradient(1100px 800px at 8% 100%, rgba(90,169,255,0.06), transparent 70%)",
        }}
      />
    </AbsoluteFill>
  );
};

const Header: React.FC<{ scene: TimedScene }> = ({ scene }) => {
  const chapter = CHAPTERS[scene.chapter];
  return (
    <>
      <div style={{ position: "absolute", left: 90, top: 50, display: "flex", gap: 14, alignItems: "center", font: `600 21px ${SANS}`, letterSpacing: 2.2, textTransform: "uppercase" }}>
        <span style={{ color: "#ff9f0a" }}>{scene.chapter === 0 ? "Silo" : `Part ${scene.chapter}`}</span>
        <span style={{ color: "#5d6068" }}>/</span>
        <span style={{ color: INK.mute }}>{chapter}</span>
      </div>
      <div style={{ position: "absolute", left: 90, top: 86, font: `650 46px ${SANS}`, color: INK.text, letterSpacing: -0.8 }}>
        {scene.title}
      </div>
    </>
  );
};

const Caption: React.FC<{ scene: TimedScene }> = ({ scene }) => {
  const frame = useCurrentFrame();
  const beat = [...scene.beats].reverse().find((candidate) => frame >= candidate.from);
  if (!beat) return null;
  const spoken = Math.round(beat.audio * 30);
  const opacity = interpolate(frame - beat.from, [0, 5, spoken + 4, spoken + 10], [0, 1, 1, 0.55], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (
    <div
      style={{
        position: "absolute",
        left: 200,
        right: 200,
        bottom: 44,
        minHeight: 84,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        font: `400 29px/1.36 ${SANS}`,
        color: INK.soft,
        opacity,
      }}
    >
      {beat.caption}
    </div>
  );
};

const Progress: React.FC<{ scene: TimedScene }> = ({ scene }) => {
  const frame = useCurrentFrame();
  const done = (scene.from + frame) / DURATION;
  const firstOfChapter = TIMELINE.filter((candidate, index) => index > 0 && candidate.chapter !== TIMELINE[index - 1].chapter);
  return (
    <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: 6, background: "#1b1e23" }}>
      <div style={{ width: `${done * 100}%`, height: "100%", background: "#ff9f0a" }} />
      {firstOfChapter.map((candidate) => (
        <span key={candidate.id} style={{ position: "absolute", left: `${(candidate.from / DURATION) * 100}%`, top: 0, width: 3, height: 6, background: INK.bg }} />
      ))}
    </div>
  );
};

/** Shown over the first second of each part, while its first picture arrives underneath. */
const ChapterCard: React.FC<{ scene: TimedScene }> = ({ scene }) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [0, 8, 44, 58], [0, 1, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  const rise = interpolate(frame, [0, 14], [18, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  if (opacity === 0) return null;
  return (
    <AbsoluteFill style={{ opacity, alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 18 }}>
      <span style={{ font: `600 30px ${SANS}`, letterSpacing: 6, textTransform: "uppercase", color: "#ff9f0a", transform: `translateY(${rise}px)` }}>
        Part {scene.chapter}
      </span>
      <span style={{ font: `700 104px ${SANS}`, letterSpacing: -2.5, color: INK.text, transform: `translateY(${rise}px)` }}>
        {CHAPTERS[scene.chapter]}
      </span>
    </AbsoluteFill>
  );
};

const SceneView: React.FC<{ scene: TimedScene; opensChapter: boolean }> = ({ scene, opensChapter }) => {
  const frame = useCurrentFrame();
  const Visual = VISUALS[scene.id];
  if (!Visual) throw new Error(`scene ${scene.id} has no picture`);
  const arrive = opensChapter ? [46, 60] : [0, 9];
  const fade = interpolate(frame, [...arrive, scene.frames - 7, scene.frames], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (
    <AbsoluteFill>
      <AbsoluteFill style={{ opacity: fade }}>
        {Visual.bare ? null : <Header scene={scene} />}
        <Stage scene={scene}>{Visual.render()}</Stage>
      </AbsoluteFill>
      {opensChapter ? <ChapterCard scene={scene} /> : null}
      <Caption scene={scene} />
      <Progress scene={scene} />
    </AbsoluteFill>
  );
};

export const Explainer: React.FC<{ narration: boolean }> = ({ narration }) => (
  <AbsoluteFill>
    <Backdrop />
    {TIMELINE.map((scene, index) => (
      <Sequence key={scene.id} from={scene.from} durationInFrames={scene.frames} name={scene.id}>
        <SceneView scene={scene} opensChapter={scene.chapter > 0 && TIMELINE[index - 1]?.chapter !== scene.chapter} />
      </Sequence>
    ))}
    {narration ? <Audio src={staticFile("narration.wav")} /> : null}
  </AbsoluteFill>
);
