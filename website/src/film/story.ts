// The film's story as data: its shots, the entries of the plan each one shows and, for a phone, where its camera
// looks and when. All times are milliseconds on one clock.

/** Every fade, and the sidebar mark's move, takes FADE. A shot starts to leave FADE before its end. */
export const FADE = 240;
/** A press comes REACT after the pointer lands, and is down and released PRESSED after that. */
export const REACT = 120;
export const PRESSED = 420;
/** How long the canvas takes to cross over when the visitor jumps. */
export const CROSSFADE = 150;

/** A part of the canvas, from its left to its right, in the canvas's units. */
export type Span = readonly [left: number, right: number];
/**
 * An eased pan to `to`, `t` ms into the shot. It runs only while nothing else moves, unless it `follows` one thing:
 * then it starts and ends with that thing's move.
 */
export interface Pan {
  t: number;
  d: number;
  to: Span;
  follows?: string;
}
export interface Shot {
  key: string;
  name: string;
  dur: number;
  /** The entries of the plan the shot shows, by id. */
  plan: readonly string[];
  /** The phone's camera. It cuts to `from` as the shot starts, when the canvas is empty; without it, it pans in. */
  camera: { from?: Span; pans: readonly Pan[] };
}

const LEFT: Span = [0, 480];
const RIGHT: Span = [508, 988];
/** Now: the commands and the spend of the runs, where the names are out of frame. */
const SPEND: Span = [190, 670];
/** The chain of the Timeline: its labels, its marks and main's line above them. */
const CHAIN: Span = [140, 620];
/** The gates of Network, with the rule each names and its verdict, and main's wires up to them. */
const GATES: Span = [50, 530];
/** The fork's right half, the wire out of it and the gate where its request is denied. */
const FORK_GATE: Span = [190, 670];

export const SHOTS = [
  {
    key: 'first', name: 'First run', dur: 8740,
    plan: ['agent-programs-moving-in', 'computers-main-on-a-mac', 'made-from-a-recipe', 'sign-in-from-a-computer'],
    camera: {
      from: LEFT,
      pans: [
        { t: 540, d: 360, to: RIGHT },
        { t: 1620, d: 280, to: [244, 724] },
        { t: 2700, d: 400, to: LEFT },
        { t: 4420, d: 420, to: RIGHT, follows: 'the first copy' },
        { t: 5760, d: 340, to: [210, 690] },
        { t: 7140, d: 360, to: RIGHT },
      ],
    },
  },
  {
    key: 'now', name: 'Now', dur: 6740,
    plan: ['now-in-the-app', 'spend-per-conversation', 'how-each-run-is-going', 'computers-macos-screen'],
    // The runs are on the left and the spend on the right: the camera goes to the spend as it arrives, comes back
    // with the line that draws the next run, goes to the screen as it opens, and ends on the runs with the stuck one.
    camera: {
      from: LEFT,
      pans: [
        { t: 620, d: 400, to: SPEND },
        { t: 1100, d: 340, to: LEFT, follows: 'the line to the second command' },
        { t: 3060, d: 300, to: SPEND },
        { t: 4120, d: 380, to: RIGHT },
        { t: 5700, d: 400, to: LEFT },
      ],
    },
  },
  {
    key: 'main', name: 'main', dur: 6940,
    plan: ['ways-out-shown', 'proposed-rules', 'rule-actions-on-the-computer'],
    camera: { from: [16, 496], pans: [{ t: 1340, d: 340, to: RIGHT }, { t: 2640, d: 280, to: [160, 640] }] },
  },
  {
    key: 'network', name: 'Network', dur: 8180,
    plan: ['secrets-held-outside', 'minimum-package-age', 'allow-hold-or-deny', 'hosts-reached-and-denied', 'network-in-the-app'],
    camera: {
      from: LEFT,
      pans: [
        { t: 540, d: 640, to: [190, 670], follows: 'the request to api.stripe.com' },
        { t: 1760, d: 800, to: RIGHT, follows: 'the request to api.stripe.com' },
        { t: 2720, d: 400, to: [316, 796] },
        { t: 4840, d: 260, to: GATES },
      ],
    },
  },
  {
    key: 'needs', name: 'Needs you', dur: 5560,
    plan: ['what-needs-you', 'alerts-on-a-change', 'questions-from-the-app'],
    camera: {
      pans: [
        { t: -FADE, d: FADE + 400, to: LEFT, follows: 'the request to paste.rs' },
        { t: 1300, d: 500, to: RIGHT, follows: 'the pointer' },
        { t: 4420, d: 440, to: LEFT },
      ],
    },
  },
  {
    key: 'timeline', name: 'Timeline', dur: 7760,
    plan: ['timeline-in-the-app', 'checkpoints-taken-on-their-own', 'processes-in-the-record', 'file-changes-in-the-record', 'what-each-run-wrote', 'denied-tool-calls'],
    camera: {
      pans: [
        { t: -FADE, d: FADE + 400, to: [120, 600], follows: 'the request to paste.rs' },
        { t: 2760, d: 300, to: [336, 816], follows: 'the branch to the files' },
        { t: 5340, d: 240, to: RIGHT, follows: 'the line to the gate' },
        { t: 6000, d: 380, to: [436, 916], follows: 'the error on its way back' },
        // The word "error" has to stay whole in view for half a second, so the camera leaves it as late as it can.
        { t: 6990, d: 250, to: CHAIN },
      ],
    },
  },
  {
    key: 'disk', name: 'Disk', dur: 5840,
    plan: ['disk-in-the-app', 'purge-of-rebuildable-files', 'how-a-path-comes-back', 'patterns-of-work'],
    camera: { from: LEFT, pans: [{ t: 1480, d: 500, to: RIGHT, follows: 'the pointer' }, { t: 2400, d: 320, to: LEFT }] },
  },
  {
    key: 'fork', name: 'fork', dur: 8840,
    plan: ['forking-a-computer', 'tasks-with-a-receipt', 'before-dropping-a-fork', 'dropping-into-the-bin', 'hosts-reached-and-denied'],
    camera: {
      from: LEFT,
      pans: [
        { t: 3780, d: 400, to: FORK_GATE, follows: 'the request to paste.rs' },
        { t: 4740, d: 360, to: LEFT },
        { t: 5200, d: 500, to: RIGHT, follows: 'notes.md' },
        { t: 5900, d: 360, to: LEFT },
        { t: 7040, d: 400, to: FORK_GATE },
      ],
    },
  },
  {
    key: 'tests', name: 'Tests', dur: 4780,
    plan: ['testing-changes-on-both-systems', 'recipes-kept-in-a-repository', 'computers-linux-beside-main'],
    camera: { from: LEFT, pans: [{ t: 1000, d: 420, to: RIGHT, follows: 'the commit' }] },
  },
  {
    key: 'ext', name: 'An extension', dur: 6640,
    plan: ['importing-an-extension', 'extension-that-protects', 'refusals-that-say-so'],
    camera: {
      from: LEFT,
      pans: [
        { t: 1000, d: 520, to: RIGHT, follows: 'the pointer' },
        { t: 2140, d: 320, to: [28, 508] },
        { t: 2900, d: 520, to: [170, 650], follows: 'the request' },
      ],
    },
  },
  {
    key: 'text', name: 'As text', dur: 3700,
    plan: ['app-as-text', 'record-of-a-computer', 'opinions-beside-the-record'],
    camera: { from: LEFT, pans: [] },
  },
] as const satisfies readonly Shot[];

export type Key = (typeof SHOTS)[number]['key'];
export interface Timed extends Shot {
  key: Key;
  start: number;
}

let total = 0;
/** The shots on the film's clock, in order. */
export const TIMED: readonly Timed[] = SHOTS.map((shot) => {
  const start = total;
  total += shot.dur;
  return { ...shot, start };
});
/** How long the loop lasts. */
export const LENGTH = total;
/** The last shot and all that has gathered around it leave together, so the loop starts from the empty window. */
export const END = LENGTH - 400;

export const shotOf = (key: Key) => TIMED.find((shot) => shot.key === key)!;
/** The shot that is on at `t`. */
export const shotAt = (t: number) => TIMED.find(({ start, dur }) => t >= start && t < start + dur)!;
export const at = (key: Key) => {
  const { start } = shotOf(key);
  return (ms: number) => start + ms;
};
/** When a shot starts to leave. */
export const out = (key: Key) => (key === 'text' ? END : shotOf(key).start + shotOf(key).dur - FADE);
/** The moment that stands for a shot when it is held. */
export const endOf = (shot: Timed) => shot.start + shot.dur - 520;
/** What the film shows before it plays, without script and with reduced motion: the end of Network. */
export const STILL = endOf(shotOf('network'));
