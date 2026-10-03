import type { CSSProperties, ReactNode } from "react";
import { createPortal } from "react-dom";
import { AbsoluteFill, Easing, interpolate, useCurrentFrame } from "remotion";
import {
  ArrowRight,
  Check,
  ChevronRight,
  Code2,
  Globe,
  Laptop,
  LockKeyhole,
  Monitor,
  Terminal as TerminalIcon,
} from "lucide-react";
import { SiloMark } from "@/components/silo-mark";
import { Preparation, RemoteHandoff } from "./preparation";
import { SshFilm } from "./ssh-film";
import { Product } from "./product";
import {
  DURATION,
  FPS,
  sceneAt,
  sourceFrameAt,
  typed,
  liveEditState,
  type SceneId,
} from "./timeline";
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const ease = (f: number, start: number, end: number, a = 0, b = 1) =>
  interpolate(f, [start, end], [a, b], clamp);

function Chrome({
  title,
  children,
  className = "",
  style,
}: {
  title: string;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <div className={`tool-window ${className}`} style={style}>
      <div className="tool-title">
        <span className="traffic">
          <i />
          <i />
          <i />
        </span>
        <span>{title}</span>
        <span className="window-dot" />
      </div>
      {children}
    </div>
  );
}
function Site({ changed = false }: { changed?: boolean }) {
  return (
    <div className="site">
      <div className="site-nav">
        <span className="site-logo">
          <SiloMark style={{ width: 22, height: 22 }} />
          hello-silo
        </span>
        <span>
          Built in a computer <span className="tiny-dot" />
        </span>
      </div>
      <div className="site-body">
        <div className="site-eyebrow">HELLO-SILO / DEVELOPMENT</div>
        <h2>{changed ? "Hello, from Office Mac." : "Hello, Silo."}</h2>
        <p>Made here. Running there.</p>
        <div className="site-button">
          View project <ArrowRight size={16} />
        </div>
      </div>
      <div className="site-bottom">
        <span>React + Vite</span>
        <span>hello-silo / main</span>
      </div>
    </div>
  );
}
function Browser({
  changed = false,
  style,
}: {
  changed?: boolean;
  style?: CSSProperties;
}) {
  return (
    <Chrome title="Safari" className="browser-window" style={style}>
      <div className="browser-toolbar">
        <span>‹ &nbsp; ›</span>
        <div>
          <LockKeyhole size={12} />
          127.0.0.1:53124<span>↻</span>
        </div>
        <span>＋</span>
      </div>
      <Site changed={changed} />
    </Chrome>
  );
}
function Editor({
  frame,
  compact = false,
}: {
  frame: number;
  compact?: boolean;
}) {
  const writing = frame >= 55;
  const edited = typed("Hello, from Office Mac.", frame, 55, 3);
  const saved = liveEditState(frame).saved;
  const code = [
    `export default function App() {`,
    `  return (`,
    `    <main className="welcome">`,
    `      <p>HELLO-SILO / DEVELOPMENT</p>`,
    `      <h1>${writing ? edited : "Hello, Silo."}</h1>`,
    `      <p>Made here. Running there.</p>`,
    `    </main>`,
    `  );`,
    `}`,
  ];
  return (
    <Chrome
      title="Zed — hello-silo"
      className={`editor-window ${compact ? "compact" : ""}`}
    >
      <div className="editor-top">
        <span>
          <Code2 size={15} />
          App.tsx {!saved && writing && <i className="unsaved" />}
        </span>
        <span>src / App.tsx</span>
      </div>
      <div className="code-body">
        {code.map((line, i) => (
          <div
            key={i}
            className={`code-line ${i === 4 && writing ? "active-line" : ""}`}
          >
            <span className="line-number">{i + 1}</span>
            <code>
              {i === 4 ? (
                <>
                  <span className="syntax-muted"> &lt;</span>
                  <span className="syntax-orange">h1</span>
                  <span className="syntax-muted">&gt;</span>
                  <span className="syntax-cream">
                    {writing ? edited : "Hello, Silo."}
                  </span>
                  {writing && !saved && (
                    <span
                      className="caret"
                      style={{ opacity: Math.floor(frame / 12) % 2 ? 0 : 1 }}
                    />
                  )}
                  <span className="syntax-muted">&lt;/h1&gt;</span>
                </>
              ) : (
                line
              )}
            </code>
          </div>
        ))}
      </div>
      <div className="editor-status">
        <span>
          ⌁ main <span className="remote-dot" /> SSH · demo @ Office Mac
        </span>
        <span>
          {saved ? (
            <>
              <Check size={13} /> Saved
            </>
          ) : (
            "TypeScript React"
          )}
        </span>
      </div>
    </Chrome>
  );
}
function DeviceTag({
  owner = false,
  children,
}: {
  owner?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className="device-tag">
      {owner ? <Monitor size={18} /> : <Laptop size={18} />}
      <strong>{owner ? "Office Mac" : "My laptop"}</strong>
      {children && <span>{children}</span>}
    </div>
  );
}
function Terminal({ frame }: { frame: number }) {
  return (
    <>
      <DeviceTag>Ghostty</DeviceTag>
      <div className="terminal-shot">
        <Chrome
          title="demo — /workspace/hello-silo"
          className="terminal-window"
        >
          <div className="terminal-context">
            <TerminalIcon size={18} />
            <span>demo</span>
            <ChevronRight size={15} />
            <span>/workspace/hello-silo</span>
            <span className="terminal-host">
              <Monitor size={16} />
              Office Mac
            </span>
          </div>
          <div className="terminal-body">
            <div className="terminal-line command">
              <span className="prompt">❯</span>{" "}
              {typed("npm run dev -- --host 0.0.0.0", frame, 5, 1.15)}
              {frame < 45 && <span className="caret" />}
            </div>
            {frame >= 48 && (
              <>
                <div className="terminal-line success">
                  VITE <span>ready</span>
                </div>
                <div className="terminal-line url">
                  ➜ Network: http://0.0.0.0:5173/
                </div>
              </>
            )}
          </div>
          <div className="terminal-bottom">
            <span className="live-dot" />
            demo @ Office Mac
          </div>
        </Chrome>
      </div>
    </>
  );
}
function ProductStage({ id, frame }: { id: SceneId; frame: number }) {
  const page =
    id === "enable" ? "connections" : (id as "connect" | "network" | "files");
  if (id === "network" && frame >= 220)
    return (
      <>
        <DeviceTag>Safari</DeviceTag>
        <div className="browser-result">
          <Browser />
        </div>
        <div className="remote-context">
          <Monitor size={18} />
          demo @ Office Mac
        </div>
      </>
    );
  const close = id === "enable" || id === "network";
  return (
    <>
      <DeviceTag owner={id === "enable"}>
        {id === "enable" ? "Allow connections from other devices" : undefined}
      </DeviceTag>
      <div className={`app-stage ${close ? "close-shot" : "wide-shot"}`}>
        <div className="app-scale">
          <Product page={page} frame={frame} />
        </div>
        {id === "connect" && frame >= 225 && (
          <div className="ownership-note" style={{ left: 530, top: 239 }}>
            <Monitor size={14} />
            Office Mac <span>Connected</span>
          </div>
        )}
        <DemoCursor id={id} frame={frame} />
      </div>
      {id === "enable" && frame >= 215 && (
        <div className="action-result">
          <Check size={19} />
          Address copied
        </div>
      )}
      {id === "connect" && frame >= 85 && frame < 115 && (
        <div className="action-result">⌘ V</div>
      )}
      {id === "connect" && frame >= 200 && (
        <div className="action-result">
          <Check size={19} />
          Office Mac connected
        </div>
      )}
    </>
  );
}
// Cursor coordinates are composition-space targets, not OS automation.
function DemoCursor({ id, frame }: { id: SceneId; frame: number }) {
  const paths: Partial<Record<SceneId, number[][]>> = {
    github: [
      [0, 700, 350],
      [20, 520, 345],
      [40, 520, 370],
      [45, 520, 370],
      [90, 1122, 435],
      [110, 1122, 435],
      [170, 1250, 550],
    ],
    secrets: [
      [0, 820, 210],
      [25, 1000, 210],
      [50, 660, 355],
      [110, 660, 355],
      [130, 1380, 458],
      [150, 1380, 458],
      [180, 1250, 520],
    ],
    backup: [
      [0, 1200, 400],
      [30, 1350, 302],
      [35, 1350, 302],
      [70, 1280, 550],
    ],
    enable: [
      [0, 180, 410],
      [65, 1392, 165],
      [90, 1392, 165],
      [150, 1355, 261],
      [215, 1355, 261],
      [245, 1380, 550],
    ],
    connect: [
      [0, 1250, 520],
      [30, 760, 178],
      [110, 760, 178],
      [150, 1309, 225],
      [185, 1309, 225],
      [225, 503, 215],
    ],
    files: [
      [0, 780, 270],
      [80, 698, 233],
      [110, 698, 233],
    ],
    network: [
      [0, 1200, 480],
      [85, 1399, 233],
      [110, 1399, 233],
      [130, 1298, 233],
      [180, 1298, 233],
      [220, 1298, 233],
    ],
  };
  const scene = sceneAt(useCurrentFrame());
  const path = paths[id];
  if (!path) return null;
  const travel =
    (8 * (scene.sourceEnd - scene.sourceStart)) /
    ((scene.end - scene.start) * FPS);
  let target = path.findIndex((p) => p[0] >= frame);
  if (target < 0) target = path.length - 1;
  const to = path[target];
  const from = path[Math.max(0, target - 1)];
  const start = Math.max(from[0], to[0] - travel);
  const progress =
    to[0] === start
      ? 1
      : interpolate(frame, [start, to[0]], [0, 1], {
          ...clamp,
          easing: Easing.out(Easing.cubic),
        });
  const x = from[1] + (to[1] - from[1]) * progress;
  const y = from[2] + (to[2] - from[2]) * progress;
  const clicks: Partial<Record<SceneId, number[]>> = {
    github: [45, 110],
    secrets: [130],
    backup: [35],
    enable: [90, 215],
    connect: [150],
    network: [110, 220],
    files: [115],
  };
  const click = (clicks[id] ?? []).find((t) => frame >= t - 12 && frame < t);
  const cursor = (
    <div
      className="demo-cursor"
      style={{
        left: x,
        top: y,
        opacity:
          (id === "github" && frame < 10) ||
          (id === "network" && frame < 65) ||
          frame > ((clicks[id] ?? []).at(-1) ?? 1000) + 20
            ? 0
            : 1,
      }}
    >
      {click !== undefined && (
        <span
          className="click-ring"
          style={{
            transform: `translate(-50%,-50%) scale(${ease(frame, click - 12, click, 1.2, 0.6)})`,
            opacity: ease(frame, click - 12, click, 0.1, 0.5),
          }}
        />
      )}
      <svg width="28" height="35" viewBox="0 0 28 35">
        <path
          d="M3 2v26l7-7 6 11 5-3-6-10h10L3 2Z"
          fill="#fff"
          stroke="#151718"
          strokeWidth="2"
          strokeLinejoin="round"
        />
      </svg>
    </div>
  );
  return id === "github"
    ? createPortal(
        <div className="repository-cursor-layer">{cursor}</div>,
        document.body,
      )
    : cursor;
}
function Opening() {
  return (
    <div className="opening">
      <div className="launch-brand">
        <SiloMark style={{ width: 150, height: 150, color: "#242424" }} />
        <span>Silo</span>
      </div>
    </div>
  );
}
function Outro() {
  return (
    <div className="outro">
      <div className="outro-brand">
        <SiloMark style={{ width: 95, height: 95, color: "#242424" }} />
        <span>Silo</span>
      </div>
      <div className="outro-link">
        github.com/amontlabs/silo <ArrowRight size={23} />
      </div>
    </div>
  );
}
export function Film() {
  const frame = useCurrentFrame();
  const scene = sceneAt(frame);
  const source = sourceFrameAt(frame);
  if (scene.id === "ssh") return <SshFilm frameOverride={source} />;
  const labels: Partial<Record<SceneId, string>> = {
    github: "GitHub access",
    secrets: "Secret destinations",
    backup: "Backup",
    enable: "Allow connections",
    connect: "Connect Office Mac",
    terminal: "Run the project",
    files: "Open the computer’s repo in Zed",
    edit: "Edit on the computer. See it live.",
    network:
      source < 35
        ? "Discover listening ports"
        : source < 110
          ? "Port 5173 detected"
          : source < 220
            ? "Forward to your laptop"
            : "Open on your laptop",
  };
  return (
    <AbsoluteFill className={`film shot-${scene.id}`}>
      {labels[scene.id] && <div className="shot-label">{labels[scene.id]}</div>}

      {scene.id === "intro" ? (
        <Opening />
      ) : scene.id === "github" ||
        scene.id === "secrets" ||
        scene.id === "backup" ? (
        <>
          <DeviceTag owner />
          <div
            className={`app-stage ${scene.id === "github" ? "wide-shot" : "close-shot"}`}
          >
            <div className="app-scale">
              <Preparation page={scene.id} frame={source} />
            </div>
            {scene.id === "github" && (
              <DemoCursor id={scene.id} frame={source} />
            )}
          </div>
        </>
      ) : scene.id === "handoff" ? (
        <RemoteHandoff frame={source} />
      ) : scene.id === "edit" ? (
        <>
          <DeviceTag />
          <div className="live-workspace">
            <div>
              <div className="live-pane-label">Zed · demo on Office Mac</div>
              <Editor frame={source} compact />
            </div>
            <div>
              <div className="live-pane-label">
                Safari · My laptop · forwarded port
              </div>
              <Browser changed={liveEditState(source).browserUpdated} />
            </div>
          </div>
          {source >= 155 && source < 180 && (
            <div className="live-save">
              ⌘ S <Check size={18} />
            </div>
          )}
        </>
      ) : scene.id === "terminal" ? (
        <Terminal frame={source} />
      ) : scene.id === "outro" ? (
        <Outro />
      ) : (
        <ProductStage id={scene.id} frame={source} />
      )}
      {scene.id !== "intro" && scene.id !== "outro" && (
        <div className="film-progress">
          <span>
            {frame < 300
              ? "01 / PREPARE"
              : frame < 600
                ? "02 / CONNECT"
                : "03 / BUILD"}
          </span>
          <div>
            <i style={{ width: `${(frame / DURATION) * 100}%` }} />
          </div>
          <SiloMark style={{ width: 24, height: 24 }} />
        </div>
      )}
    </AbsoluteFill>
  );
}
