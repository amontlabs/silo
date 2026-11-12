import React, { createContext, useContext, useMemo } from "react";
import { Easing, getInputProps, interpolate, spring, useCurrentFrame } from "remotion";
import type { LucideIcon } from "lucide-react";
import { FPS, HEIGHT, WIDTH, type TimedScene } from "./timeline.ts";

/**
 * One meaning per colour, for the whole film: orange is Silo's engine, blue is
 * the trusted device, violet is the untrusted computer and its agent, green is
 * allowed, red is blocked or a known gap.
 */
export const TONES = {
  plain: { line: "#4a4f57", fill: "#191c21", ink: "#f3eee4" },
  engine: { line: "#ff9f0a", fill: "#2a1f0c", ink: "#ffc266" },
  device: { line: "#5aa9ff", fill: "#0f1f33", ink: "#9fcbff" },
  guest: { line: "#b58cff", fill: "#1f1733", ink: "#d2bcff" },
  ok: { line: "#3ecf8e", fill: "#0e2a20", ink: "#8be9bf" },
  bad: { line: "#ff6b6b", fill: "#331414", ink: "#ffa3a3" },
  ghost: { line: "#34383d", fill: "transparent", ink: "#a09b90" },
} as const;
export type Tone = keyof typeof TONES;

export const INK = { text: "#f3eee4", soft: "#c9c4b8", mute: "#8f8a80", bg: "#0d0f12" };
export const MONO = '"SF Mono", Menlo, Monaco, monospace';
export const SANS = '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Helvetica Neue", sans-serif';

type Rect = { x: number; y: number; w: number; h: number };

type SceneState = {
  scene: TimedScene;
  /** Index of the sentence being spoken. */
  index: number;
  /** Beat keys in which some element is spotlighted. */
  spot: Set<string>;
  rects: Map<string, Rect>;
};

const SceneContext = createContext<SceneState | null>(null);

const useSceneState = () => {
  const state = useContext(SceneContext);
  if (!state) throw new Error("explainer element used outside a Stage");
  return state;
};

/** `"key"` or `"key+12"`: a beat, optionally some frames after it starts. */
const parseAt = (at: string) => {
  const [key, offset] = at.split("+");
  return { key, offset: offset ? Number(offset) : 0 };
};

const beatStart = (scene: TimedScene, at: string | undefined) => {
  if (!at) return 0;
  const { key, offset } = parseAt(at);
  const beat = scene.beats.find((candidate) => candidate.key === key);
  if (!beat) throw new Error(`scene ${scene.id} has no beat "${key}"`);
  return beat.from + offset;
};

/** Frame at which a beat starts, for hand-written motion. */
export const useBeat = () => {
  const { scene, index } = useSceneState();
  const frame = useCurrentFrame();
  return {
    frame,
    key: scene.beats[index]?.key ?? "",
    start: (at: string) => beatStart(scene, at),
    /** 0 before the beat, easing to 1 over `frames` after it starts. */
    progress: (at: string, frames = 18) =>
      interpolate(frame - beatStart(scene, at), [0, frames], [0, 1], {
        extrapolateLeft: "clamp",
        extrapolateRight: "clamp",
        easing: Easing.out(Easing.cubic),
      }),
    on: (at: string) => frame >= beatStart(scene, at),
    /** True while any of the named beats is being spoken. */
    during: (keys: string) => keys.split(" ").includes(scene.beats[index]?.key ?? ""),
  };
};

const hotKeys = (at: string | undefined, hot: string | undefined): string[] => {
  if (hot === "*" || hot === "-") return [];
  if (hot) return hot.split(" ");
  return at ? [parseAt(at).key] : [];
};

/** Entrance (0 to 1) and spotlight (0 dimmed, 0.5 resting, 1 lit) of one element. */
const usePresence = (at: string | undefined, hot: string | undefined) => {
  const { scene, index, spot } = useSceneState();
  const frame = useCurrentFrame();
  const start = beatStart(scene, at);
  const enter = spring({ frame: frame - start, fps: FPS, config: { damping: 200, stiffness: 170 } });
  const keys = hotKeys(at, hot);
  const level = (beatIndex: number) => {
    const key = scene.beats[beatIndex]?.key;
    if (hot === "*" || !key || !spot.has(key)) return 0.5;
    return keys.includes(key) ? 1 : 0;
  };
  const beat = scene.beats[index];
  const blend = beat
    ? interpolate(frame - beat.from, [0, 9], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })
    : 1;
  const light = level(index - 1) * (1 - blend) + level(index) * blend;
  return { enter, light, visible: frame >= start };
};

/** Layout review renders every element at full strength, whatever the sentence. */
const FLAT = getInputProps().flat === true;
const dimming = (light: number) => (FLAT ? 1 : interpolate(light, [0, 0.5, 1], [0.5, 1, 1]));

/** Walk a scene's elements to learn where its nodes sit and which beats spotlight something. */
const survey = (children: React.ReactNode, rects: Map<string, Rect>, spot: Set<string>) => {
  React.Children.forEach(children, (child) => {
    if (!React.isValidElement(child)) return;
    const props = child.props as Record<string, unknown>;
    if (typeof props.id === "string" && typeof props.x === "number" && typeof props.w === "number") {
      rects.set(props.id, { x: props.x, y: props.y as number, w: props.w, h: props.h as number });
    }
    // Frames and plain text never take the spotlight unless a scene says so.
    const quiet = child.type === Group || ((child.type === Text || child.type === Code) && props.hot === undefined);
    if (!quiet) {
      for (const key of hotKeys(props.at as string | undefined, props.hot as string | undefined)) spot.add(key);
    }
    for (const rows of [props.items, props.rows]) {
      if (!Array.isArray(rows)) continue;
      for (const row of rows as (Timed | string)[]) {
        if (typeof row !== "string") for (const key of hotKeys(row.at, row.hot)) spot.add(key);
      }
    }
    if (props.children) survey(props.children as React.ReactNode, rects, spot);
  });
};

export const Stage: React.FC<{ scene: TimedScene; children: React.ReactNode }> = ({ scene, children }) => {
  const frame = useCurrentFrame();
  const state = useMemo(() => {
    const rects = new Map<string, Rect>();
    const spot = new Set<string>();
    survey(children, rects, spot);
    return { rects, spot };
  }, [children]);
  let index = 0;
  scene.beats.forEach((beat, beatIndex) => {
    if (frame >= beat.from) index = beatIndex;
  });
  return (
    <SceneContext.Provider value={{ scene, index, ...state }}>
      <div style={{ position: "absolute", inset: 0, width: WIDTH, height: HEIGHT }}>{children}</div>
    </SceneContext.Provider>
  );
};

type Timed = { at?: string; hot?: string };

export const Node: React.FC<
  Timed &
    Rect & {
      id?: string;
      title: string;
      sub?: string;
      icon?: LucideIcon;
      tone?: Tone;
      mono?: boolean;
      size?: number;
      align?: "center" | "left";
    }
> = ({ at, hot, x, y, w, h, title, sub, icon: Icon, tone = "plain", mono, size, align = "center" }) => {
  const { enter, light, visible } = usePresence(at, hot);
  if (!visible) return null;
  const colors = TONES[tone];
  const lit = Math.max(0, light * 2 - 1);
  const titleSize = size ?? (h < 70 ? 22 : 26);
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        height: h,
        boxSizing: "border-box",
        borderRadius: 16,
        border: `2px solid ${colors.line}`,
        background: colors.fill,
        boxShadow: `0 0 ${34 * lit}px ${colors.line}${lit > 0.05 ? "66" : "00"}, 0 10px 30px rgba(0,0,0,0.35)`,
        opacity: enter * dimming(light),
        transform: `translateY(${(1 - enter) * 14}px) scale(${0.96 + enter * 0.04})`,
        display: "flex",
        flexDirection: "column",
        alignItems: align === "center" ? "center" : "flex-start",
        justifyContent: "center",
        textAlign: align,
        padding: "8px 18px",
        gap: 5,
        zIndex: 2,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
        {Icon ? <Icon size={titleSize + 2} color={colors.ink} strokeWidth={2} /> : null}
        <span
          style={{
            font: `600 ${titleSize}px ${mono ? MONO : SANS}`,
            color: tone === "plain" ? INK.text : colors.ink,
            letterSpacing: mono ? 0 : -0.2,
            whiteSpace: "nowrap",
          }}
        >
          {title}
        </span>
      </div>
      {sub ? (
        <span style={{ font: `400 ${Math.min(22, titleSize - 4)}px ${SANS}`, color: INK.soft, lineHeight: 1.25, whiteSpace: "pre-line" }}>
          {sub}
        </span>
      ) : null}
    </div>
  );
};

/** A labelled boundary. It frames other elements and is never dimmed. */
export const Group: React.FC<Timed & Rect & { label: string; tone?: Tone; id?: string; solid?: boolean }> = ({
  at,
  x,
  y,
  w,
  h,
  label,
  tone = "ghost",
  solid,
}) => {
  const { enter, visible } = usePresence(at, "*");
  if (!visible) return null;
  const colors = TONES[tone];
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        height: h,
        boxSizing: "border-box",
        borderRadius: 22,
        border: `2px ${solid ? "solid" : "dashed"} ${colors.line}${tone === "ghost" ? "" : "aa"}`,
        background: tone === "ghost" ? "rgba(255,255,255,0.015)" : `${colors.line}0d`,
        opacity: enter,
        zIndex: 0,
      }}
    >
      <span
        style={{
          position: "absolute",
          left: 22,
          top: -15,
          padding: "0 12px",
          background: INK.bg,
          font: `600 20px ${SANS}`,
          letterSpacing: 1.4,
          textTransform: "uppercase",
          color: colors.ink,
        }}
      >
        {label}
      </span>
    </div>
  );
};

type Side = "l" | "r" | "t" | "b";
type Point = { x: number; y: number; side?: Side };

const anchor = (rect: Rect, side: Side, shift = 0): Point => {
  if (side === "l") return { x: rect.x, y: rect.y + rect.h / 2 + shift, side };
  if (side === "r") return { x: rect.x + rect.w, y: rect.y + rect.h / 2 + shift, side };
  if (side === "t") return { x: rect.x + rect.w / 2 + shift, y: rect.y, side };
  return { x: rect.x + rect.w / 2 + shift, y: rect.y + rect.h, side };
};

const NORMAL: Record<Side, [number, number]> = { l: [-1, 0], r: [1, 0], t: [0, -1], b: [0, 1] };

/** `"id"`, `"id.r"` or `"id.r+20"` (shifted along the side), or a raw point. */
const resolveEnds = (rects: Map<string, Rect>, from: string | Point, to: string | Point): [Point, Point] => {
  const parse = (end: string | Point) => {
    if (typeof end !== "string") return { point: end };
    const match = /^([^.]+)(?:\.([lrtb]))?([+-]\d+)?$/.exec(end);
    const rect = match ? rects.get(match[1]) : undefined;
    if (!match || !rect) throw new Error(`edge end "${end}" names no node`);
    return { rect, side: match[2] as Side | undefined, shift: match[3] ? Number(match[3]) : 0 };
  };
  const a = parse(from);
  const b = parse(to);
  const centre = (end: ReturnType<typeof parse>) =>
    end.point ?? { x: end.rect!.x + end.rect!.w / 2, y: end.rect!.y + end.rect!.h / 2 };
  const ca = centre(a);
  const cb = centre(b);
  const horizontal = Math.abs(cb.x - ca.x) >= Math.abs(cb.y - ca.y);
  const auto = (towards: Point, self: Point): Side =>
    horizontal ? (towards.x >= self.x ? "r" : "l") : towards.y >= self.y ? "b" : "t";
  const pa = a.point ?? anchor(a.rect!, a.side ?? auto(cb, ca), a.shift);
  const pb = b.point ?? anchor(b.rect!, b.side ?? auto(ca, cb), b.shift);
  return [pa, pb];
};

const cubic = (p0: Point, p1: Point, p2: Point, p3: Point, t: number) => {
  const u = 1 - t;
  return {
    x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
    y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y,
  };
};

export const Edge: React.FC<
  Timed & {
    from: string | Point;
    to: string | Point;
    label?: string;
    tone?: Tone;
    dashed?: boolean;
    /** Dots travel along the edge while it is spotlighted (or always, with "always"). */
    flow?: boolean | "always";
    /** A refused route: red, dashed, crossed out, no arrowhead. */
    blocked?: boolean;
    both?: boolean;
    /** Where along the edge the label sits, 0 to 1. */
    labelAt?: number;
    labelShift?: [number, number];
  }
> = ({ at, hot, from, to, label, tone = "plain", dashed, flow, blocked, both, labelAt = 0.5, labelShift = [0, 0] }) => {
  const { rects } = useSceneState();
  const { enter, light, visible } = usePresence(at, hot);
  const frame = useCurrentFrame();
  if (!visible) return null;
  const [a, b] = resolveEnds(rects, from, to);
  const distance = Math.hypot(b.x - a.x, b.y - a.y);
  const reach = Math.min(140, distance * 0.42);
  const na = a.side ? NORMAL[a.side] : [0, 0];
  const nb = b.side ? NORMAL[b.side] : [0, 0];
  const c1 = { x: a.x + na[0] * reach, y: a.y + na[1] * reach };
  const c2 = { x: b.x + nb[0] * reach, y: b.y + nb[1] * reach };
  const path = `M ${a.x} ${a.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${b.x} ${b.y}`;
  const colors = TONES[blocked ? "bad" : tone];
  const stroke = tone === "plain" && !blocked ? "#7d8088" : colors.line;
  const lit = Math.max(0, light * 2 - 1);
  const length = distance * 1.25 + 40;
  const drawn = interpolate(enter, [0, 1], [length, 0]);
  const middle = cubic(a, c1, c2, b, labelAt);
  const head = (tip: Point, control: Point) => {
    const angle = Math.atan2(tip.y - control.y, tip.x - control.x);
    const wing = (delta: number) =>
      `${tip.x - 15 * Math.cos(angle + delta)},${tip.y - 15 * Math.sin(angle + delta)}`;
    return <polygon points={`${tip.x},${tip.y} ${wing(0.42)} ${wing(-0.42)}`} fill={stroke} opacity={enter} />;
  };
  const flowing = flow === "always" || (flow && lit > 0.2);
  const dots = flowing
    ? [0, 1, 2].map((dot) => {
        const t = ((frame / 42 + dot / 3) % 1 + 1) % 1;
        const point = cubic(a, c1, c2, b, t);
        return <circle key={dot} cx={point.x} cy={point.y} r={6} fill={colors.ink} opacity={Math.sin(t * Math.PI) * 0.95} />;
      })
    : null;
  return (
    <>
      <svg
        width={WIDTH}
        height={HEIGHT}
        style={{ position: "absolute", inset: 0, opacity: dimming(light), zIndex: 1, overflow: "visible" }}
      >
        <path
          d={path}
          fill="none"
          stroke={stroke}
          strokeWidth={3 + lit}
          strokeLinecap="round"
          strokeDasharray={dashed || blocked ? "10 10" : `${length} ${length}`}
          strokeDashoffset={dashed || blocked ? 0 : drawn}
          opacity={dashed || blocked ? enter : 1}
        />
        {blocked ? null : head(b, c2)}
        {both && !blocked ? head(a, c1) : null}
        {dots}
        {blocked ? (
          <g opacity={enter} transform={`translate(${middle.x} ${middle.y})`}>
            <circle r={19} fill="#331414" stroke={colors.line} strokeWidth={2.5} />
            <path d="M -7 -7 L 7 7 M 7 -7 L -7 7" stroke={colors.line} strokeWidth={3.5} strokeLinecap="round" />
          </g>
        ) : null}
      </svg>
      {label ? (
        <div
          style={{
            position: "absolute",
            left: middle.x + labelShift[0],
            top: middle.y + labelShift[1] + (blocked ? 34 : 0),
            transform: "translate(-50%, -50%)",
            padding: "3px 12px",
            borderRadius: 9,
            background: INK.bg,
            border: `1px solid ${stroke}55`,
            font: `500 19px ${SANS}`,
            color: blocked ? colors.ink : tone === "plain" ? INK.soft : colors.ink,
            whiteSpace: "nowrap",
            opacity: enter * dimming(light),
            zIndex: 3,
          }}
        >
          {label}
        </div>
      ) : null}
    </>
  );
};

/** Free text. `size` picks a preset; `tone` colours it. */
export const Text: React.FC<
  Timed & {
    x: number;
    y: number;
    w?: number;
    children: React.ReactNode;
    size?: number;
    tone?: Tone;
    weight?: number;
    mono?: boolean;
    align?: "left" | "center" | "right";
    soft?: boolean;
  }
> = ({ at, hot = "*", x, y, w, children, size = 26, tone, weight = 400, mono, align = "left", soft }) => {
  const { enter, light, visible } = usePresence(at, hot);
  if (!visible) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        font: `${weight} ${size}px/1.32 ${mono ? MONO : SANS}`,
        color: tone ? TONES[tone].ink : soft ? INK.mute : INK.text,
        textAlign: align,
        whiteSpace: "pre-line",
        opacity: enter * dimming(light),
        transform: `translateY(${(1 - enter) * 10}px)`,
        zIndex: 3,
      }}
    >
      {children}
    </div>
  );
};

export type Item = { at?: string; hot?: string; text: string; sub?: string; tone?: Tone; icon?: LucideIcon };

/** A list whose rows arrive one by one. */
export const List: React.FC<{ x: number; y: number; w: number; items: Item[]; gap?: number; size?: number }> = ({
  x,
  y,
  w,
  items,
  gap = 16,
  size = 27,
}) => (
  <div style={{ position: "absolute", left: x, top: y, width: w, display: "flex", flexDirection: "column", gap, zIndex: 3 }}>
    {items.map((item) => (
      <Row key={item.text} item={item} size={size} />
    ))}
  </div>
);

const Row: React.FC<{ item: Item; size: number }> = ({ item, size }) => {
  const { enter, light, visible } = usePresence(item.at, item.hot);
  const colors = TONES[item.tone ?? "plain"];
  const Icon = item.icon;
  return (
    <div
      style={{
        display: "flex",
        gap: 16,
        alignItems: "flex-start",
        opacity: visible ? enter * dimming(light) : 0,
        transform: `translateX(${(1 - enter) * -14}px)`,
      }}
    >
      {Icon ? (
        <Icon size={size + 3} color={colors.ink} style={{ flex: "none", marginTop: 3 }} />
      ) : (
        <span
          style={{
            flex: "none",
            width: 11,
            height: 11,
            borderRadius: 6,
            marginTop: size * 0.42,
            background: item.tone ? colors.line : "#7d8088",
          }}
        />
      )}
      <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <span style={{ font: `500 ${size}px/1.28 ${SANS}`, color: item.tone ? colors.ink : INK.text }}>{item.text}</span>
        {item.sub ? <span style={{ font: `400 ${size - 6}px/1.3 ${SANS}`, color: INK.mute }}>{item.sub}</span> : null}
      </div>
    </div>
  );
};

export type CodeLine = { at?: string; text: string; tone?: Tone; soft?: boolean };

/** A terminal or file panel whose lines arrive with their beats. */
export const Code: React.FC<Timed & Rect & { title?: string; lines: CodeLine[]; size?: number }> = ({
  at,
  hot = "*",
  x,
  y,
  w,
  h,
  title,
  lines,
  size = 22,
}) => {
  const { scene } = useSceneState();
  const { enter, light, visible } = usePresence(at, hot);
  const frame = useCurrentFrame();
  if (!visible) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        height: h,
        boxSizing: "border-box",
        borderRadius: 16,
        border: "2px solid #34383d",
        background: "#101216",
        overflow: "hidden",
        opacity: enter * dimming(light),
        transform: `translateY(${(1 - enter) * 14}px)`,
        zIndex: 2,
      }}
    >
      <div style={{ height: 44, display: "flex", alignItems: "center", gap: 9, padding: "0 18px", background: "#181b20", borderBottom: "1px solid #2a2e34" }}>
        {["#ff6b6b", "#ffbd4a", "#3ecf8e"].map((dot) => (
          <span key={dot} style={{ width: 12, height: 12, borderRadius: 6, background: dot, opacity: 0.75 }} />
        ))}
        <span style={{ marginLeft: 10, font: `500 18px ${MONO}`, color: INK.mute }}>{title}</span>
      </div>
      <div style={{ padding: "16px 22px", display: "flex", flexDirection: "column", gap: 5 }}>
        {lines.map((line, index) => {
          const shown = interpolate(frame - beatStart(scene, line.at ?? at), [0, 10], [0, 1], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          });
          return (
            <span
              key={`${index}-${line.text}`}
              style={{
                font: `400 ${size}px/1.4 ${MONO}`,
                color: line.tone ? TONES[line.tone].ink : line.soft ? INK.mute : INK.soft,
                whiteSpace: "pre",
                opacity: shown,
              }}
            >
              {line.text || " "}
            </span>
          );
        })}
      </div>
    </div>
  );
};

/** A small rounded tag. */
export const Chip: React.FC<Timed & { x: number; y: number; text: string; tone?: Tone; mono?: boolean; size?: number }> = ({
  at,
  hot,
  x,
  y,
  text,
  tone = "plain",
  mono,
  size = 20,
}) => {
  const { enter, light, visible } = usePresence(at, hot);
  if (!visible) return null;
  const colors = TONES[tone];
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        padding: "5px 14px",
        borderRadius: 999,
        border: `1.5px solid ${colors.line}`,
        background: colors.fill === "transparent" ? INK.bg : colors.fill,
        font: `500 ${size}px ${mono ? MONO : SANS}`,
        color: tone === "plain" ? INK.soft : colors.ink,
        whiteSpace: "nowrap",
        opacity: enter * dimming(light),
        transform: `scale(${0.9 + enter * 0.1})`,
        zIndex: 3,
      }}
    >
      {text}
    </div>
  );
};

/** A simple table: a header row and rows that arrive with their beats. */
export const Grid: React.FC<{
  x: number;
  y: number;
  columns: number[];
  head: string[];
  rows: { at?: string; hot?: string; cells: (string | { text: string; tone: Tone })[] }[];
  size?: number;
  rowHeight?: number;
}> = ({ x, y, columns, head, rows, size = 23, rowHeight = 58 }) => {
  const width = columns.reduce((total, column) => total + column, 0);
  return (
    <div style={{ position: "absolute", left: x, top: y, width, zIndex: 2 }}>
      <div style={{ display: "flex", height: 46, alignItems: "center", borderBottom: "2px solid #34383d" }}>
        {head.map((cell, index) => (
          <span key={cell} style={{ width: columns[index], font: `600 19px ${SANS}`, letterSpacing: 1.2, textTransform: "uppercase", color: INK.mute, paddingLeft: 14, boxSizing: "border-box" }}>
            {cell}
          </span>
        ))}
      </div>
      {rows.map((row, index) => (
        <GridRow key={index} row={row} columns={columns} size={size} height={rowHeight} />
      ))}
    </div>
  );
};

const GridRow: React.FC<{
  row: { at?: string; hot?: string; cells: (string | { text: string; tone: Tone })[] };
  columns: number[];
  size: number;
  height: number;
}> = ({ row, columns, size, height }) => {
  const { enter, light, visible } = usePresence(row.at, row.hot);
  return (
    <div
      style={{
        display: "flex",
        minHeight: height,
        alignItems: "center",
        borderBottom: "1px solid #24282d",
        opacity: visible ? enter * dimming(light) : 0,
        transform: `translateY(${(1 - enter) * 8}px)`,
      }}
    >
      {row.cells.map((cell, index) => {
        const text = typeof cell === "string" ? cell : cell.text;
        const tone = typeof cell === "string" ? undefined : cell.tone;
        return (
          <span
            key={index}
            style={{
              width: columns[index],
              boxSizing: "border-box",
              padding: "8px 14px",
              font: `${index === 0 ? 600 : 400} ${size}px/1.25 ${SANS}`,
              color: tone ? TONES[tone].ink : index === 0 ? INK.text : INK.soft,
              whiteSpace: "pre-line",
            }}
          >
            {text}
          </span>
        );
      })}
    </div>
  );
};

/** Tags that wrap onto as many lines as they need, arriving one after another. */
export const Chips: React.FC<
  Timed & { x: number; y: number; w: number; items: string[]; tone?: Tone; mono?: boolean; size?: number; stagger?: number }
> = ({ at, hot, x, y, w, items, tone = "plain", mono, size = 21, stagger = 2 }) => {
  const { scene } = useSceneState();
  const { light, visible } = usePresence(at, hot);
  const frame = useCurrentFrame();
  if (!visible) return null;
  const colors = TONES[tone];
  const start = beatStart(scene, at);
  return (
    <div style={{ position: "absolute", left: x, top: y, width: w, display: "flex", flexWrap: "wrap", gap: 10, opacity: dimming(light), zIndex: 3 }}>
      {items.map((item, index) => {
        const enter = spring({ frame: frame - start - index * stagger, fps: FPS, config: { damping: 200, stiffness: 170 } });
        return (
          <span
            key={item}
            style={{
              padding: "6px 15px",
              borderRadius: 999,
              border: `1.5px solid ${colors.line}`,
              background: colors.fill === "transparent" ? INK.bg : colors.fill,
              font: `500 ${size}px ${mono ? MONO : SANS}`,
              color: tone === "plain" ? INK.soft : colors.ink,
              whiteSpace: "nowrap",
              opacity: enter,
              transform: `translateY(${(1 - enter) * 8}px)`,
            }}
          >
            {item}
          </span>
        );
      })}
    </div>
  );
};
