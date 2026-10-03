import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { AbsoluteFill, Sequence, continueRender, delayRender, Easing, interpolate, useCurrentFrame } from 'remotion';
import { Archive, ArrowRight, Check, Code2, GitBranch, Globe, KeyRound, Laptop, Monitor, MousePointer2, ShieldCheck, Terminal } from 'lucide-react';
import { ApplicationPreview } from '@/fixtures/application-preview';
import { LinuxDesktopViewer } from '@/desktop/linux-desktop-viewer';
import { SiloMark } from '@/components/silo-mark';
import { showcaseSource, showcaseDirectoryLoader } from './showcase-fixtures';
import { agentTaskAt, releaseScenes } from './release-timeline';
import { ReleaseBackup } from './release-backup';
import { ReleaseSsh } from './release-ssh';
import { computerTarget } from '@/features/application/model/connections';
import './release-style.css';

const clamp = { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' } as const;
const move = (f: number, start: number, duration: number, a = 0, b = 1) => interpolate(f, [start, start + duration], [a, b], { ...clamp, easing: Easing.bezier(.22, 1, .36, 1) });
const linear = (f: number, start: number, duration: number, a = 0, b = 1) => interpolate(f, [start, start + duration], [a, b], clamp);
const noop = () => undefined;
const copyStyle = (f: number, delay = 0): CSSProperties => ({ opacity: move(f, delay, 22), transform: `translateY(${move(f, delay, 30, 35, 0)}px)` });

function Brand({ large = false }: { large?: boolean }) {
  return <div className={`r-brand ${large ? 'r-brand-large' : ''}`}><SiloMark /><span>Silo</span></div>;
}

function Frame({ children, index, label, light = false, note = 'SILO / LINUX COMPUTERS' }: { children: ReactNode; index?: string; label?: string; light?: boolean; note?: string }) {
  return <AbsoluteFill className={`r-frame ${light ? 'r-light' : ''}`}>
    <div className="r-grid" />
    <div className="r-top"><Brand /><span>{label ?? 'A SPACE OF THEIR OWN'}</span></div>
    {children}
    <div className="r-bottom"><span>{note}</span><span>{index ?? 'RELEASE FILM'}<span className="r-cross">+</span></span></div>
  </AbsoluteFill>;
}

function Heading({ children, sub, frame, style }: { children: ReactNode; sub?: ReactNode; frame: number; style?: CSSProperties }) {
  return <div className="r-heading" style={{ ...style, ...copyStyle(frame, 3) }}><h1>{children}</h1>{sub && <p style={copyStyle(frame, 12)}>{sub}</p>}</div>;
}

function ProductWindow({ page, width = 1180, height = 580, style, frame = 0 }: { page: 'overview' | 'files' | 'github' | 'secrets' | 'network'; width?: number; height?: number; style?: CSSProperties; frame?: number }) {
  const surface = useRef<HTMLDivElement>(null);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);
  const editSecret = page === 'secrets' && frame >= 35;
  const portConnected = frame >= 62;
  useLayoutEffect(() => {
    if (!editSecret) return;
    const handle = delayRender('Open the production secret editor');
    surface.current?.querySelector<HTMLButtonElement>('button[aria-label="Edit PACKAGE_TOKEN"]')?.click();
    let second = 0;
    const first = requestAnimationFrame(() => { second = requestAnimationFrame(() => continueRender(handle)); });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); continueRender(handle); };
  }, [editSecret]);
  useLayoutEffect(() => {
    if (page !== 'network') return;
    const handle = delayRender('Locate the production port action');
    const selector = portConnected ? '[aria-label="Open http://127.0.0.1:53124 in Safari"]' : '[aria-label="Connect port 5173 to this computer"]';
    let second = 0;
    const first = requestAnimationFrame(() => {
      const button = surface.current?.querySelector<HTMLButtonElement>(selector);
      const root = surface.current?.getBoundingClientRect();
      const box = button?.getBoundingClientRect();
      if (root && box) setPointer({ x: (box.x + box.width / 2 - root.x) * width / root.width, y: (box.y + box.height / 2 - root.y) * height / root.height });
      second = requestAnimationFrame(() => continueRender(handle));
    });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); continueRender(handle); };
  }, [page, portConnected, width, height]);
  const source = {
    ...showcaseSource,
    secrets: [{ id: 'package-token', name: 'PACKAGE_TOKEN', computers: ['web'], allowedDomains: ['registry.npmjs.org'], state: 'active' as const }],
    ...(page === 'network' ? { network: { computers: [{ computer: computerTarget(showcaseSource.computers[2]), error: null, ports: [{ port: 5173, hostPort: portConnected ? 53124 : null, scheme: 'http' as const, state: portConnected ? 'reachable' as const : 'unpublished' as const, configured: portConnected }] }] } } : {}),
  };
  return <div ref={surface} className="r-product" style={{ width, height, ...style }}>
    <ApplicationPreview key={`${page}-${editSecret}`} source={source} initialRoute={page === 'github' || page === 'secrets' ? { tab: page } : { tab: 'computers', computerSection: page }} actions={{ listComputerDirectory: showcaseDirectoryLoader }} />
    {page === 'network' && pointer && frame >= 18 && frame < 105 && <Cursor x={move(frame, 18, 24, pointer.x - 130, pointer.x)} y={move(frame, 18, 24, pointer.y + 100, pointer.y)} click={frame >= 94 ? linear(frame, 94, 11) : frame >= 62 && frame < 78 ? linear(frame, 62, 16) : 0} />}
  </div>;
}

function Opening() {
  const f = useCurrentFrame();
  return <Frame label="INTRODUCING SILO" note="YOUR HARDWARE. YOUR COMPUTERS.">
    <div className="r-opening-title">
      <div style={copyStyle(f, 5)}>A computer.</div>
      <div style={copyStyle(f, 20)}>For your <em>agents.</em></div>
      <p style={copyStyle(f, 45)}>Linux computers on the devices you own.</p>
    </div>
    <div className="r-orbit" style={{ opacity: move(f, 0, 25), transform: `translateY(${move(f, 0, 40, 50, 0)}px) scale(${move(f, 0, 60, .86, 1)})` }}>
      <svg viewBox="0 0 500 500"><g fill="none" strokeWidth="18" strokeLinecap="round">
        {[{ r: 195, dash: '1000 225', angle: -28, color: '#edece5' }, { r: 130, dash: '630 185', angle: 63, color: '#edece5' }, { r: 65, dash: '290 120', angle: 154, color: '#ff9f0a' }].map((v, i) => <circle key={v.r} cx="250" cy="250" r={v.r} stroke={v.color} strokeDasharray={v.dash} transform={`rotate(${v.angle + move(f, i * 7, 80, (i % 2 ? -1 : 1) * 110, 0)} 250 250)`} />)}
      </g></svg>
      <div className="r-orbit-caption" style={copyStyle(f, 65)}>INDEPENDENT BY DESIGN</div>
    </div>
    <div className="r-opening-rule" style={{ transform: `scaleX(${move(f, 22, 65)})` }} />
  </Frame>;
}

function Cursor({ x, y, click = 0 }: { x: number; y: number; click?: number }) {
  return <div className="r-cursor" style={{ left: x, top: y }}>
    {click > 0 && <i style={{ transform: `translate(-50%, -50%) scale(${1 + click * 1.8})`, opacity: 1 - click }} />}
    <MousePointer2 size={30} fill="#f6f2e9" stroke="#202c29" strokeWidth={1.7} />
  </div>;
}

function AgentDesktop({ frame }: { frame: number }) {
  const state = agentTaskAt(frame);
  const cursorX = move(frame, 78, 29, 1010, 730);
  const cursorY = move(frame, 78, 29, 386, 337);
  return <div className="r-desktop">
    <LinuxDesktopViewer name="web · Linux desktop" state={{ installed: true, state: 'running', autoStart: true }} busy={false} error={null} onAction={noop} onRetry={noop} onFullscreen={noop} />
    <div className="r-guest">
      <div className="r-guest-panel"><span>Applications</span><span>web · Linux desktop</span></div>
      <div className="r-guest-wallpaper"><div /><div /></div>
      <div className="r-guest-browser" style={{ transform: `translateY(${move(frame, 8, 32, 36, 0)}px)`, opacity: move(frame, 8, 24) }}>
        <div className="r-native-title">Project Hub — Mozilla Firefox<span>−　□　×</span></div>
        <div className="r-url"><span>‹　›　↻</span><div><Globe size={13} />localhost:3000</div></div>
        <div className="r-project-site"><div className="r-site-logo"><span>p.</span> PROJECT HUB</div><h2>Room for<br/><i>your next idea.</i></h2><p>One place for your team's next project.</p><div className={`r-create ${frame >= 112 && frame < 124 ? 'is-pressed' : ''}`}>Create a project <ArrowRight size={17} /></div>
          <div className="r-created-card" style={{ opacity: move(frame, 148, 18), transform: `translateY(${move(frame, 148, 24, 18, 0)}px)` }}><div><Check size={18} /><strong>Website redesign</strong><span>Just created</span></div><div className="r-card-progress"><i /></div></div>
        </div>
      </div>
      <div className="r-agent-terminal" style={{ opacity: move(frame, 0, 20), transform: `translateX(${move(frame, 0, 30, -35, 0)}px)` }}>
        <div className="r-native-title">Agent terminal<span>−　□　×</span></div>
        <div className="r-agent-body"><div className="r-mono-label">EXAMPLE AGENT SESSION</div><p><b>›</b> Open the app and test<br/>　the project creation flow.</p><div className="r-agent-tool"><MousePointer2 size={15} /> LCU · computer use</div>
          {[{ ok: state.observe, text: 'Observe the Linux desktop' }, { ok: state.click, text: 'Click “Create a project”' }, { ok: state.created, text: 'Inspect the result' }].map(row => <div className="r-task-line" key={row.text} style={{ opacity: row.ok ? 1 : .26 }}><Check size={16} />{row.text}</div>)}
          <div className="r-task-result" style={{ opacity: move(frame, 190, 18) }}><Check size={17} /> Project creation verified.</div>
        </div>
      </div>
      {frame > 55 && frame < 148 && <Cursor x={cursorX} y={cursorY} click={frame >= 112 ? linear(frame, 112, 18) : 0} />}
    </div>
  </div>;
}

function DesktopScene() {
  const f = useCurrentFrame();
  const focus = move(f, 188, 45);
  return <Frame index="01 / 08" label="COMPUTER USE" note="ILLUSTRATED AGENT SESSION · PRODUCTION SILO VIEWER">
    <Heading frame={f}>A desktop they can <em>use.</em></Heading>
    <div className="r-desktop-stage" style={{ opacity: move(f, 0, 18), transform: `translateY(${move(f, 0, 35, 80, 0)}px) scale(${1 + focus * .035})` }}><AgentDesktop frame={f} /></div>
    <div className="r-desktop-caption" style={copyStyle(f, 215)}><span>Observe.</span><span>Act.</span><span className="r-amber">Check the result.</span></div>
  </Frame>;
}

function ComputersScene() {
  const f = useCurrentFrame();
  return <Frame index="02 / 08" label="LOCAL + REMOTE">
    <Heading frame={f}>Your devices. <em>One place.</em></Heading>
    <div className="r-computer-map">
      <div className="r-host" style={copyStyle(f, 12)}><Laptop size={38} strokeWidth={1.3} /><div><strong>This device</strong><span>web · services</span></div><i /></div>
      <div className="r-link"><span style={{ height: `${move(f, 35, 45, 0, 100)}%` }} /><b style={copyStyle(f, 52)}>SSH</b></div>
      <div className="r-host r-remote-host" style={copyStyle(f, 60)}><Monitor size={38} strokeWidth={1.3} /><div><strong>Studio Mac</strong><span>lab</span></div><i /></div>
      <p style={copyStyle(f, 92)}>Start, stop, and manage<br/>local and remote computers.</p>
    </div>
    <div className="r-fleet-stage" style={{ opacity: move(f, 9, 24), transform: `translateX(${move(f, 9, 40, 120, 0)}px)` }}><ProductWindow page="overview" width={1040} height={530} /></div>
    <div className="r-fleet-footnote" style={copyStyle(f, 115)}><span className="r-live-dot" />3 computers. 2 devices. All yours.</div>
  </Frame>;
}

function Editor({ frame }: { frame: number }) {
  const typed = 'Made on my computer.'.slice(0, Math.max(0, Math.floor((frame - 55) / 2)));
  return <div className="r-editor">
    <div className="r-native-title">Zed — web-app<span>−　□　×</span></div>
    <div className="r-editor-tab"><Code2 size={16} /> App.tsx <span>src / App.tsx</span></div>
    <div className="r-code">
      {['export default function App() {','  return (','    <main>',`      <h1>${frame < 55 ? 'Hello, Silo.' : typed}</h1>`,'      <p>Built here. Ready to share.</p>','    </main>','  );','}'].map((text, i) => <div key={i} className={i === 3 ? 'r-code-active' : ''}><span>{i + 1}</span><code>{text}</code></div>)}
    </div>
    <div className="r-editor-status"><span><GitBranch size={14} /> feat/dashboard</span><span>{frame > 98 ? '✓ Saved' : 'TypeScript React'}</span></div>
  </div>;
}

function ToolsScene() {
  const f = useCurrentFrame() * 1.3;
  return <Frame index="03 / 08" label="FAMILIAR TOOLS">
    <Heading frame={f}>Your tools. <em>Your flow.</em></Heading>
    <div className="r-tools-product" style={{ opacity: move(f, 0, 18), transform: `translateX(${move(f, 0, 38, -100, 0)}px)` }}><ProductWindow page="files" width={1030} height={545} /></div>
    <div className="r-tools-editor" style={{ opacity: move(f, 38, 22), transform: `translateY(${move(f, 38, 36, 90, 0)}px)` }}><Editor frame={f} /></div>
    <div className="r-terminal-strip" style={copyStyle(f, 124)}><Terminal size={23} /><span>web /workspace/web-app</span><code>npm run dev -- --host 0.0.0.0</code><span className="r-ready"><i />Ready</span></div>
    <div className="r-tools-caption" style={copyStyle(f, 148)}>Open your project. Keep your editor and terminal.</div>
  </Frame>;
}

function AccessScene({ secrets = false }: { secrets?: boolean }) {
  const f = useCurrentFrame();
  return <Frame index={secrets ? '06 / 08' : '05 / 08'} label="ACCESS YOU CONTROL">
    <div className="r-access-copy"><div className="r-icon-box" style={copyStyle(f)}>{secrets ? <KeyRound size={34} strokeWidth={1.4} /> : <GitBranch size={34} strokeWidth={1.4} />}</div>
      <h1 style={copyStyle(f, 5)}>{secrets ? <>The right<br/>credentials.<br/><em>In scope.</em></> : <>The right<br/>repositories.<br/><em>You decide.</em></>}</h1>
      <p style={copyStyle(f, 22)}>{secrets ? <>Choose the computers and<br/>HTTPS domains each secret can use.</> : <>Choose repositories per computer.<br/>Read-only by default with OAuth.</>}</p>
    </div>
    <div className={`r-access-stage ${secrets ? 'r-secrets-stage' : ''}`} style={{ opacity: move(f, 0, 20), transform: `translateX(${move(f, 0, 36, 90, 0)}px)` }}><ProductWindow page={secrets ? 'secrets' : 'github'} width={1060} height={670} frame={f} /></div>
    <div className="r-access-detail" style={copyStyle(f, 55)}>{secrets ? <><KeyRound size={20} /><span>PACKAGE_TOKEN</span><ArrowRight size={18} /><strong>registry.npmjs.org</strong></> : <><ShieldCheck size={23} /><span>example/design-system</span><strong>Read-only</strong></>}</div>
  </Frame>;
}

function SshScene() {
  const f = useCurrentFrame();
  return <Frame index="04 / 08" label="SSH TO YOUR COMPUTER" note="ILLUSTRATED CLIENT · EXISTING NETWORK ROUTE REQUIRED">
    <Heading frame={f}>Your agents. <em>Connected.</em></Heading>
    <ReleaseSsh frame={f} />
  </Frame>;
}

function BackupScene() {
  const f = useCurrentFrame();
  return <Frame index="07 / 08" label="BACKUP + RESTORE" note="TIME-COMPRESSED EXAMPLE · LOCAL COMPUTER BACKUP">
    <div className="r-access-copy"><div className="r-icon-box" style={copyStyle(f)}><Archive size={34} strokeWidth={1.4} /></div>
      <h1 style={copyStyle(f, 5)}>Keep a copy.<br/><em>Keep going.</em></h1>
      <p style={copyStyle(f, 22)}>Back up local computers.<br/>Restore as a new computer.</p>
    </div>
    <div className="r-access-stage" style={{ opacity: move(f, 0, 20), transform: `translateX(${move(f, 0, 36, 90, 0)}px)` }}><ReleaseBackup frame={f} /></div>
    <div className="r-access-detail" style={copyStyle(f, 105)}><Archive size={22} /><span>web · managed disks + settings</span><Check size={21} /></div>
  </Frame>;
}

function PreviewBrowser({ frame }: { frame: number }) {
  return <div className="r-preview-browser"><div className="r-native-title">Safari<span>−　□　×</span></div><div className="r-url"><span>‹　›　↻</span><div><Globe size={15} />127.0.0.1:53124</div></div><div className="r-preview-site"><div className="r-preview-nav"><span><SiloMark />hello-silo</span><span>DEVELOPMENT</span></div><span className="r-mono-label">BUILT IN A SANDBOX</span><h2>Made here.<br/><em>Running there.</em></h2><p>Your app, running on Studio Mac.<br/>Open in the browser on your laptop.</p><div className="r-preview-button">Hello, Silo. <ArrowRight size={23} /></div><span className="r-preview-live" style={copyStyle(frame, 32)}><i /> Connected through a local address</span></div></div>;
}

function PreviewScene() {
  const f = useCurrentFrame();
  return <Frame index="08 / 08" label="BUILD → OPEN → SEE">
    <Heading frame={f}>Build there. <em>Open here.</em></Heading>
    <div className="r-network-stage" style={{ opacity: move(f, 0, 18), transform: `translateX(${move(f, 0, 32, -60, 0)}px)` }}><ProductWindow page="network" frame={f} width={1215} height={562.5} style={{ transform: 'scale(1.15)', border: 0 }} /></div>
    <div className="r-preview-stage" style={{ opacity: move(f, 100, 20), transform: `translateY(${move(f, 100, 38, 90, 0)}px)` }}><PreviewBrowser frame={f - 100} /></div>
    <div className="r-port-route" style={copyStyle(f, 130)}><Monitor size={24} /><span>Studio Mac :5173</span><span className="r-route-line" /><Laptop size={24} /><span>Your browser</span></div>
  </Frame>;
}

function Closing() {
  const f = useCurrentFrame();
  return <Frame light label="AVAILABLE FOR macOS + LINUX" note="OPEN SOURCE · MIT LICENSE">
    <div className="r-close-lockup" style={copyStyle(f, 5)}><Brand large /></div>
    <div className="r-close-title" style={copyStyle(f, 15)}>Give your agents<br/><em>a space of their own.</em></div>
    <div className="r-close-cta" style={copyStyle(f, 32)}><span>silo.amontlabs.com</span><ArrowRight size={35} strokeWidth={1.5} /></div>
    <div className="r-close-tags" style={copyStyle(f, 45)}><span><Monitor size={19} />Linux computers</span><span><MousePointer2 size={19} />Agent desktops</span><span><KeyRound size={19} />Scoped access</span></div>
  </Frame>;
}

export function ReleaseFilm() {
  const [handle] = useState(() => delayRender('Load release typography'));
  useLayoutEffect(() => {
    const wasDark = document.documentElement.classList.contains('dark');
    document.documentElement.classList.add('dark');
    Promise.all([document.fonts.load('500 20px "Release Sans"'), document.fonts.load('italic 20px "Release Serif"'), document.fonts.load('400 20px "Release Mono"')]).then(() => continueRender(handle));
    return () => { if (!wasDark) document.documentElement.classList.remove('dark'); };
  }, [handle]);
  const components = { opening: <Opening />, desktop: <DesktopScene />, computers: <ComputersScene />, tools: <ToolsScene />, ssh: <SshScene />, github: <AccessScene />, secrets: <AccessScene secrets />, backup: <BackupScene />, preview: <PreviewScene />, closing: <Closing /> };
  return <AbsoluteFill className="r-film">{releaseScenes.map(scene => <Sequence key={scene.id} from={scene.from} durationInFrames={scene.duration}>{components[scene.id]}</Sequence>)}</AbsoluteFill>;
}
