// The map of the Workshop: where each part of plan/parts.json sits on the full map and on the folded one, the zones
// around them, the text that is not a part and the wires between them. Units are 1/1136 of the map's width; the full
// map is 620 units high, the fold 180. The build checks the layout against plan/parts.json (`checkLayout`).
// map-example.ts shows every kind of thing a layout can hold.
import { titleProblems } from './map-fit.ts';

export type Box = [x: number, y: number, w: number, h: number];

/**
 * What the plan says that a wire, a zone or a placement draws, read only by the build (`checkLayout`): the id of a
 * decision or a feature, `word:<id>` for a word of plan/words.json, or `part:<id>` for what only the part's own line
 * says. A taken decision, a taken or built feature, a word and a part are in force. A decision with an answer and no
 * verdict, and a feature with no verdict, are proposed: what rests on one of them is drawn as not decided.
 */
export type Rests = string[];

/** A small drawing inside a part, in its own viewBox, placed `at` [left, top, width] in map units from the part's corner. */
export interface Glyph {
  viewBox: string;
  at: [left: number, top: number, width: number];
  /** Drawn as lines; `solid` fills the shape, `core` takes the accent when the part is done or being built. */
  paths: { d: string; kind?: 'solid' | 'core' }[];
}

export interface Place {
  full: Box;
  fold: Box;
  /** Drawn inside another part: smaller title, above its parent. */
  inner?: boolean;
  /** Tall enough that its folded title sits at the top, not in the middle. */
  tall?: boolean;
  /** A bar: the state is written on the title's line, after it. */
  wide?: boolean;
  /** Title and state line offsets for short boxes. */
  ty?: number;
  sy?: number;
  glyph?: Glyph;
  rests?: Rests;
  undecided?: boolean;
}

export const MAP_WIDTH = 1136;
export const MAP_HEIGHT = 620;
export const FOLD_HEIGHT = 180;

export interface Zone {
  label: string;
  full: Box;
  fold: Box;
  corners?: boolean;
  rounded?: boolean;
  /** Every zone has it; the build checks it. */
  rests?: Rests;
  undecided?: boolean;
}

/**
 * Text on the map that is not a part: its title, and a line the folded map hides. The line wraps within the box's
 * width. A text with no `fold` leaves the folded map: it fades out where it stands on the full one.
 */
export interface Note {
  title: string;
  line: string;
  full: Box;
  fold?: Box;
  rests?: Rests;
  undecided?: boolean;
}

/** A copy stacked behind a part, coloured by that part's state. */
export interface Ghost {
  of: string;
  full: Box;
  fold: Box;
  rests?: Rests;
  undecided?: boolean;
}

export type Arrow = [x: number, y: number, dir: 'r' | 'l' | 'u' | 'd'];

/** The lines of a wire on one map: its path, its arrows, and a cross at the end where the wire stops. */
export interface Drawing {
  d: string;
  arrows?: Arrow[];
  cross?: [x: number, y: number];
}

/**
 * A wire of the full map. `fold` draws the same relation on the folded map, so it looks like this wire there: the same
 * part's colour, and dashed when this wire is not decided. A wire with no `fold` leaves the folded map.
 */
export interface Wire extends Drawing {
  /** The part whose state colours the wire. */
  of: string;
  label?: [x: number, y: number, text: string, anchor?: 'middle'];
  fold?: Drawing;
  /** Every wire has it; the build checks it. */
  rests?: Rests;
  undecided?: boolean;
}

export interface MapLayout {
  /** The file that holds the layout, as the build's messages name it. */
  file?: string;
  /** Every part of plan/parts.json by id, in Tab order: roughly left to right, then top to bottom. */
  places: Record<string, Place>;
  zones: Zone[];
  notes: Note[];
  ghosts: Ghost[];
  wires: Wire[];
  /** Where the key sits on the full map, [left, top]: what a solid and a dashed line mean. The folded map hides it. */
  key?: [x: number, y: number];
}

/** A layout that draws no map: with it any plan builds, and the page shows the parts as tiles only. */
export const NO_MAP: MapLayout = { places: {}, zones: [], notes: [], ghosts: [], wires: [] };

/**
 * Silo's map: your Mac, and inside it the virtual machines where the computers run. The app and the engine sit on the
 * left, the record, the opinions and the rules along the bottom, the boundary on the edge of the virtual machines, and
 * outside the Mac the hosts a computer reaches, and the servers and clouds where computers also run later. The wires
 * follow a request out through the boundary to the internet, the calls to model providers that Model traffic and spend
 * reads on the way, the facts the boundary leaves in the record, what the record triggers back at the boundary, the
 * record shown in the app, and your own terminal reaching a computer over SSH. The boundary's channels are written
 * under it. What rests on a proposal (the channels, SSH, servers and clouds) is drawn as not decided until it is taken.
 */
export const SILO_MAP: MapLayout = {
  file: 'website/src/workshop/map.ts',
  places: {
    views: { full: [48, 114, 136, 144], fold: [36, 30, 120, 30] },
    plugins: { full: [48, 418, 160, 64], fold: [36, 136, 128, 26], ty: 13, sy: 36 },
    engine: { full: [204, 114, 112, 272], fold: [172, 30, 96, 96], tall: true },
    opinions: { full: [232, 418, 204, 64], fold: [180, 136, 172, 26], ty: 13, sy: 36 },
    recipes: { full: [456, 42, 168, 44], fold: [420, 14, 132, 26], ty: 6, sy: 26 },
    computers: { full: [372, 130, 180, 128], fold: [300, 58, 216, 34] },
    'agent-programs': { full: [388, 186, 136, 52], fold: [384, 63, 108, 24], inner: true, ty: 7, sy: 29 },
    'going-back': { full: [372, 298, 152, 56], fold: [300, 98, 104, 26], ty: 9, sy: 31 },
    record: { full: [480, 418, 160, 64], fold: [456, 136, 100, 26], ty: 13, sy: 36 },
    boundary: { full: [592, 162, 124, 64], fold: [532, 62, 100, 28], ty: 13, sy: 36, rests: ['word:boundary'] },
    rules: { full: [688, 418, 184, 64], fold: [580, 136, 160, 26], ty: 13, sy: 36, rests: ['where-precautions-live'] },
    // Above the way out: it reads the calls to model providers, not everything that leaves.
    traffic: { full: [744, 82, 196, 64], fold: [656, 22, 164, 28], ty: 13, sy: 36 },
    // A low bar under the whole drawing: what holds in every part. Folded, it ends where it ends on the full map, low
    // enough that it passes under Rules and precautions on its way there.
    checking: { full: [24, 566, 1088, 48], fold: [944, 142, 168, 26], ty: 6, sy: 27, rests: ['properties-are-checked'] },
  },
  zones: [
    { label: 'YOUR MAC', full: [24, 18, 936, 536], fold: [16, 8, 828, 164], corners: true, rests: ['who-it-is-for', 'where-it-runs'] },
    { label: 'VIRTUAL MACHINES', full: [348, 106, 296, 280], fold: [288, 48, 276, 82], rounded: true, rests: ['part:computers'] },
  ],
  notes: [
    { title: 'Your terminal', line: 'and your editor', full: [326, 44, 124, 40], rests: ['computer-reached-over-ssh', 'you-visit-a-computer', 'reaching-a-computer'] },
    { title: 'A server or a cloud', line: 'where computers also run, later', full: [990, 21, 122, 52], rests: ['where-it-runs', 'many-forks-elsewhere'] },
    { title: 'Model providers', line: 'some of the hosts', full: [990, 96, 122, 40], fold: [878, 27, 144, 20], rests: ['part:traffic'] },
    { title: 'The internet', line: 'every other host', full: [990, 200, 122, 40], fold: [878, 71, 144, 20] },
    { title: 'The boundary\'s channels', line: 'the network, ports, files, secrets, the screen, the clipboard, repository access', full: [728, 240, 212, 96], rests: ['channel-list'] },
  ],
  // Main is one computer among several: forks and fresh computers.
  ghosts: [
    { of: 'computers', full: [384, 142, 180, 128], fold: [300, 58, 216, 34], rests: ['part:computers'] },
    { of: 'computers', full: [378, 136, 180, 128], fold: [300, 58, 216, 34], rests: ['part:computers'] },
  ],
  wires: [
    { of: 'views', d: 'M184 178H197', arrows: [[204, 178, 'r']], rests: ['engine-language'], fold: { d: 'M156 45H165', arrows: [[172, 45, 'r']] } },
    { of: 'computers', d: 'M316 194H365', arrows: [[372, 194, 'r']], label: [332, 186, 'runs', 'middle'], rests: ['macos-vm-driver'], fold: { d: 'M268 75H293', arrows: [[300, 75, 'r']] } },
    // Later the engine also runs computers on a server or in a cloud.
    { of: 'computers', d: 'M230 114V30H977', arrows: [[984, 30, 'r']], label: [640, 46, 'runs'], rests: ['linux-server-host', 'forks-on-a-cloud-machine'] },
    // Your own terminal and editor reach a computer over SSH, from the Mac.
    { of: 'agent-programs', d: 'M336 88V150H365', arrows: [[372, 150, 'r']], label: [303, 104, 'SSH'], rests: ['computer-reached-over-ssh', 'you-visit-a-computer', 'reaching-a-computer'] },
    { of: 'recipes', d: 'M540 86V123', arrows: [[540, 130, 'd']], label: [549, 99, 'makes'], rests: ['what-a-recipe-sets-up'], fold: { d: 'M496 40V51', arrows: [[496, 58, 'd']] } },
    // A request leaves the computer through the boundary, which applies the network policy, to a host of the internet.
    {
      of: 'boundary', d: 'M564 194H585M716 209H977', arrows: [[592, 194, 'r'], [984, 209, 'r']], label: [850, 201, 'requests', 'middle'],
      rests: ['network-policy-by-the-user', 'first-seen-host', 'denied-attempts', 'what-was-sent-to-a-host'],
      fold: { d: 'M516 76H525M632 80H865', arrows: [[532, 76, 'r'], [872, 80, 'r']] },
    },
    // The calls to a model provider leave the same way, and Model traffic and spend reads them.
    {
      of: 'traffic', d: 'M716 178H790V153M940 105H977', arrows: [[790, 146, 'u'], [984, 105, 'r']], label: [799, 168, 'calls'],
      rests: ['part:boundary', 'part:traffic', 'tool-calls-from-traffic'],
      fold: { d: 'M632 68H700V57M820 36H865', arrows: [[700, 50, 'u'], [872, 36, 'r']] },
    },
    { of: 'record', d: 'M620 226V411', arrows: [[620, 418, 'd']], label: [578, 330, 'facts'], rests: ['how-silo-knows'], fold: { d: 'M544 90V129', arrows: [[544, 136, 'd']] } },
    { of: 'opinions', d: 'M480 450H443', arrows: [[436, 450, 'l']], label: [458, 442, 'read', 'middle'], rests: ['what-the-record-holds'], fold: { d: 'M456 149H359', arrows: [[352, 149, 'l']] } },
    // A rule is triggered by a fact, a precaution by an opinion.
    { of: 'rules', d: 'M640 450H681M334 482V510H720V489', arrows: [[688, 450, 'r'], [720, 482, 'u']], label: [527, 528, 'triggers', 'middle'], rests: ['word:rule', 'word:precaution', 'how-an-opinion-acts'], fold: { d: 'M556 149H573', arrows: [[580, 149, 'r']] } },
    { of: 'rules', d: 'M760 418V344H680V233', arrows: [[680, 226, 'u']], label: [769, 384, 'decides'], rests: ['where-limits-are-set', 'what-a-precaution-may-do'], fold: { d: 'M600 136V97', arrows: [[600, 90, 'u']] } },
    // The app shows facts, reports and opinions, each as what it is; facts and reports are the record.
    { of: 'views', d: 'M500 418V400H150V265', arrows: [[150, 258, 'u']], label: [159, 330, 'shown'], rests: ['kinds-look-different', 'word:record'] },
  ],
  key: [984, 492],
};

export function arrowPath([x, y, dir]: Arrow): string {
  const s = 3.6;
  const l = 7;
  switch (dir) {
    case 'r': return `M${x} ${y}l${-l} ${-s}v${2 * s}z`;
    case 'l': return `M${x} ${y}l${l} ${-s}v${2 * s}z`;
    case 'd': return `M${x} ${y}l${-s} ${-l}h${2 * s}z`;
    default: return `M${x} ${y}l${-s} ${l}h${2 * s}z`;
  }
}

export function crossPath([x, y]: [number, number]): string {
  return `M${x - 4.5} ${y - 4.5}l9 9M${x + 4.5} ${y - 4.5}l-9 9`;
}

/** CSS custom properties that put a box at its full and folded place. */
export function placeStyle(full: Box, fold: Box, place?: Pick<Place, 'ty' | 'sy'>): Record<string, string | number> {
  return {
    '--x': full[0], '--y': full[1], '--w': full[2], '--h': full[3],
    '--cx': fold[0], '--cy': fold[1], '--cw': fold[2], '--ch': fold[3],
    ...(place?.ty !== undefined ? { '--ty': place.ty } : {}),
    ...(place?.sy !== undefined ? { '--sy': place.sy } : {}),
  };
}

/** Whether a layout draws a map: it places at least one part. */
export const hasMap = (layout: MapLayout) => Object.keys(layout.places).length > 0;

export class LayoutError extends Error {}

/** What `checkLayout` and `pageLayout` read of the plan: its parts, the status of each decision and feature, and its words. */
export interface LayoutPlan {
  parts: { id: string; title: string }[];
  /** By id; `replacedBy` is the decision that replaced a replaced one. */
  decisions: Record<string, { status: 'dropped' | 'replaced' | 'taken' | 'answered' | 'open'; replacedBy: string | null }>;
  /** By id, as the plan tool's model gives it. */
  features: Record<string, { status: 'built' | 'ready' | 'taken' | 'undecided' | 'dropped' }>;
  /** The ids of the words of plan/words.json. */
  words: string[];
}

/** What an id of `rests` names: something in force, something proposed, or a problem that stops the build. */
type Standing = { in: 'force' } | { in: 'proposal' } | { problem: string };

function standing(id: string, plan: LayoutPlan, words: Set<string>, parts: Set<string>): Standing {
  const force: Standing = { in: 'force' };
  const proposal: Standing = { in: 'proposal' };
  if (id.startsWith('word:')) return words.has(id.slice(5)) ? force : { problem: `the word "${id}", which plan/words.json does not have` };
  if (id.startsWith('part:')) return parts.has(id.slice(5)) ? force : { problem: `the part "${id}", which plan/parts.json does not have` };
  const decision = Object.hasOwn(plan.decisions, id) ? plan.decisions[id] : undefined;
  if (decision) {
    switch (decision.status) {
      case 'taken': return force;
      case 'answered': return proposal;
      case 'open': return { problem: `the decision "${id}", which is open: nobody has answered it, so nothing says what to draw` };
      case 'dropped': return { problem: `the decision "${id}", which was dropped` };
      default: return { problem: `the decision "${id}", which was replaced by "${decision.replacedBy}"` };
    }
  }
  const feature = Object.hasOwn(plan.features, id) ? plan.features[id] : undefined;
  if (feature) {
    if (feature.status === 'dropped') return { problem: `the feature "${id}", which was dropped` };
    return feature.status === 'undecided' ? proposal : force;
  }
  return { problem: `"${id}", which is no decision or feature of the plan` };
}

/** Whether a thing that rests on `rests` is drawn as not decided: at least one of them is proposed. */
function undecided(rests: Rests | undefined, plan: LayoutPlan): boolean {
  const words = new Set(plan.words);
  const parts = new Set(plan.parts.map((part) => part.id));
  return (rests ?? []).some((id) => {
    const found = standing(id, plan, words, parts);
    return 'in' in found && found.in === 'proposal';
  });
}

/**
 * The layout as the page gets it: its file and what each thing rests on are the build's alone. A thing that rests on
 * something proposed gets `undecided`; nothing else of `rests` is kept. A layout with no file and no `rests` is
 * returned as it is.
 */
export function pageLayout(layout: MapLayout, plan: LayoutPlan): MapLayout {
  const things = [...layout.wires, ...layout.zones, ...layout.notes, ...layout.ghosts, ...Object.values(layout.places)];
  if (layout.file === undefined && !things.some((thing) => thing.rests)) return layout;
  const drawn = <T extends { rests?: Rests; undecided?: boolean }>({ rests, undecided: _, ...rest }: T): Omit<T, 'rests'> =>
    (undecided(rests, plan) ? { ...rest, undecided: true } : rest) as Omit<T, 'rests'>;
  return {
    places: Object.fromEntries(Object.entries(layout.places).map(([id, place]) => [id, drawn(place)])),
    zones: layout.zones.map(drawn),
    notes: layout.notes.map(drawn),
    ghosts: layout.ghosts.map(drawn),
    wires: layout.wires.map(drawn),
    ...(layout.key ? { key: layout.key } : {}),
  };
}

/** Each thing of the layout that says what it rests on, named as a message names it; `must` when it needs `rests`. */
function restingThings(layout: MapLayout): { name: string; noun: string; rests: Rests | undefined; must: boolean }[] {
  return [
    ...layout.wires.map((wire) => ({ name: wire.label ? `the wire "${wire.label[2]}"` : `the wire with no word, drawn "${wire.d}"`, noun: 'wire', rests: wire.rests, must: true })),
    ...layout.zones.map((zone) => ({ name: `the zone "${zone.label}"`, noun: 'zone', rests: zone.rests, must: true })),
    ...Object.entries(layout.places).map(([id, place]) => ({ name: `the place of the part "${id}"`, noun: 'place', rests: place.rests, must: false })),
    ...layout.notes.map((note) => ({ name: `the text "${note.title}"`, noun: 'text', rests: note.rests, must: false })),
    ...layout.ghosts.map((ghost, index) => ({ name: `copy ${index + 1} behind the part "${ghost.of}"`, noun: 'copy', rests: ghost.rests, must: false })),
  ];
}

/**
 * An empty layout passes. Otherwise every part has a place on the map, and the map places, wires and stacks copies for
 * only parts that exist; every wire and every zone rests on something of the plan, and nothing rests on an id that
 * names nothing, an open, dropped or replaced decision, or a dropped feature; and every part's title fits its box.
 * Throws a LayoutError that names each problem and its fix.
 */
export function checkLayout(plan: LayoutPlan, layout: MapLayout = SILO_MAP): void {
  // The messages name the file of the layout checked: Silo's map, or the tests' example.
  const where = layout.file ?? 'the map\'s layout';
  const partIds = plan.parts.map((part) => part.id);
  const known = new Set(partIds);
  // An empty layout is no map at all: the page shows the tiles alone, and any plan passes.
  if (!hasMap(layout) && !layout.zones.length && !layout.notes.length && !layout.ghosts.length && !layout.wires.length) return;
  for (const id of partIds) {
    // Own keys only: a part named like a property every object has (`constructor`) has no place unless the layout gives it one.
    if (!Object.hasOwn(layout.places, id)) throw new LayoutError(`The map has no place for the part "${id}" of plan/parts.json. Add it to ${where}.`);
  }
  for (const id of Object.keys(layout.places)) {
    if (!known.has(id)) throw new LayoutError(`${where} places the part "${id}", which plan/parts.json does not have.`);
  }
  for (const wire of layout.wires) {
    if (!known.has(wire.of)) throw new LayoutError(`${where} draws a wire for the part "${wire.of}", which plan/parts.json does not have.`);
  }
  for (const ghost of layout.ghosts) {
    if (!known.has(ghost.of)) throw new LayoutError(`${where} stacks a copy behind the part "${ghost.of}", which plan/parts.json does not have.`);
  }
  const words = new Set(plan.words);
  const problems: string[] = [];
  for (const thing of restingThings(layout)) {
    if (thing.must && !thing.rests?.length) {
      problems.push(`${where}: ${thing.name} rests on nothing. Fix: give it \`rests\`: the decisions and features of the plan it draws, words as word:<id>, or part:<id> for what only a part's line says.`);
      continue;
    }
    for (const id of thing.rests ?? []) {
      const found = standing(id, plan, words, known);
      if ('problem' in found) problems.push(`${where}: ${thing.name} rests on ${found.problem}. Fix: look at the ${thing.noun}, then name what it rests on now.`);
    }
  }
  problems.push(...titleProblems(plan.parts, layout, where));
  if (problems.length) throw new LayoutError(problems.join('\n'));
}
