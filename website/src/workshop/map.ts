// The map of the Workshop: where each part of plan/parts.json sits on the full map and on the folded one, the zones
// around them, the text that is not a part and the wires between them. Units are 1/1136 of the map's width; the full
// map is 620 units high, the fold 180. The build checks the layout against plan/parts.json (`checkLayout`).
// map-example.ts shows every kind of thing a layout can hold.

export type Box = [x: number, y: number, w: number, h: number];

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
}

/** Text on the map that is not a part: its title, and a line the folded map hides. */
export interface Note {
  title: string;
  line: string;
  full: Box;
  fold: Box;
}

/** A copy stacked behind a part, coloured by that part's state. */
export interface Ghost {
  of: string;
  full: Box;
  fold: Box;
}

export type Arrow = [x: number, y: number, dir: 'r' | 'l' | 'u' | 'd'];

export interface Wire {
  /** The part whose state colours the wire. */
  of: string;
  d: string;
  arrows?: Arrow[];
  label?: [x: number, y: number, text: string, anchor?: 'middle'];
  /** A cross at the end: where the wire stops. */
  cross?: [x: number, y: number];
}

export interface MapLayout {
  /** Every part of plan/parts.json by id, in Tab order: roughly left to right, then top to bottom. */
  places: Record<string, Place>;
  zones: Zone[];
  notes: Note[];
  ghosts: Ghost[];
  /** The wires of the full map, and those of the folded one. */
  wires: { full: Wire[]; fold: Wire[] };
}

/** A layout that draws no map: with it any plan builds, and the page shows the parts as tiles only. */
export const NO_MAP: MapLayout = { places: {}, zones: [], notes: [], ghosts: [], wires: { full: [], fold: [] } };

/**
 * Silo's map: the Mac, and inside it the virtual machines where the computers run. The app and the engine sit on the
 * left, the record, the opinions and the rules along the bottom, the boundary on the edge of the virtual machines and
 * the model providers outside the Mac. The wires follow a request out through the boundary, the facts it leaves into
 * the record, and what the record triggers back at the boundary.
 */
export const SILO_MAP: MapLayout = {
  places: {
    views: { full: [48, 114, 136, 144], fold: [36, 30, 120, 30] },
    plugins: { full: [48, 418, 160, 64], fold: [36, 136, 128, 26], ty: 13, sy: 36 },
    engine: { full: [204, 114, 112, 272], fold: [172, 30, 96, 96], tall: true },
    opinions: { full: [232, 418, 204, 64], fold: [180, 136, 172, 26], ty: 13, sy: 36 },
    recipes: { full: [456, 34, 168, 48], fold: [420, 14, 132, 26], ty: 6, sy: 27 },
    computers: { full: [372, 130, 180, 128], fold: [300, 58, 216, 34] },
    'agent-programs': { full: [388, 186, 136, 52], fold: [384, 63, 104, 24], inner: true, ty: 7, sy: 29 },
    'going-back': { full: [372, 298, 152, 56], fold: [300, 98, 104, 26], ty: 9, sy: 31 },
    record: { full: [480, 418, 160, 64], fold: [456, 136, 100, 26], ty: 13, sy: 36 },
    boundary: { full: [592, 162, 124, 64], fold: [532, 62, 100, 28], ty: 13, sy: 36 },
    rules: { full: [688, 418, 184, 64], fold: [580, 136, 160, 26], ty: 13, sy: 36 },
    traffic: { full: [744, 162, 196, 64], fold: [656, 62, 164, 28], ty: 13, sy: 36 },
    // A low bar under the whole drawing: what holds in every part.
    checking: { full: [24, 566, 1088, 48], fold: [872, 124, 168, 30], ty: 6, sy: 27 },
  },
  zones: [
    { label: 'YOUR MAC', full: [24, 18, 936, 536], fold: [16, 8, 828, 164], corners: true },
    { label: 'VIRTUAL MACHINES', full: [348, 106, 296, 280], fold: [288, 48, 276, 82], rounded: true },
  ],
  notes: [{ title: 'Model providers', line: 'outside Silo', full: [984, 40, 128, 40], fold: [872, 58, 150, 36] }],
  // Main is one computer among several: forks and fresh computers.
  ghosts: [
    { of: 'computers', full: [384, 142, 180, 128], fold: [300, 58, 216, 34] },
    { of: 'computers', full: [378, 136, 180, 128], fold: [300, 58, 216, 34] },
  ],
  wires: {
    full: [
      { of: 'views', d: 'M184 178H197', arrows: [[204, 178, 'r']] },
      { of: 'computers', d: 'M316 194H365', arrows: [[372, 194, 'r']], label: [332, 186, 'runs', 'middle'] },
      { of: 'recipes', d: 'M540 82V123', arrows: [[540, 130, 'd']], label: [549, 98, 'makes'] },
      // A request leaves the computer through the boundary, where its model traffic is read, to the model provider.
      { of: 'traffic', d: 'M564 194H585M716 194H737M842 162V49H977', arrows: [[592, 194, 'r'], [744, 194, 'r'], [984, 49, 'r']], label: [851, 112, 'calls'] },
      { of: 'record', d: 'M620 226V411', arrows: [[620, 418, 'd']], label: [629, 330, 'facts'] },
      { of: 'opinions', d: 'M480 450H443', arrows: [[436, 450, 'l']], label: [458, 442, 'read', 'middle'] },
      // A rule is triggered by a fact, a precaution by an opinion.
      { of: 'rules', d: 'M640 450H681M334 482V510H720V489', arrows: [[688, 450, 'r'], [720, 482, 'u']], label: [527, 528, 'triggers', 'middle'] },
      { of: 'rules', d: 'M760 418V300H680V233', arrows: [[680, 226, 'u']], label: [769, 362, 'decides'] },
    ],
    fold: [
      { of: 'views', d: 'M156 45H165', arrows: [[172, 45, 'r']] },
      { of: 'computers', d: 'M268 75H293', arrows: [[300, 75, 'r']] },
      { of: 'recipes', d: 'M496 40V51', arrows: [[496, 58, 'd']] },
      { of: 'traffic', d: 'M516 76H525M632 76H649M820 76H865', arrows: [[532, 76, 'r'], [656, 76, 'r'], [872, 76, 'r']] },
      { of: 'record', d: 'M544 90V129', arrows: [[544, 136, 'd']] },
      { of: 'opinions', d: 'M456 149H359', arrows: [[352, 149, 'l']] },
      { of: 'rules', d: 'M556 149H573', arrows: [[580, 149, 'r']] },
      { of: 'rules', d: 'M600 136V97', arrows: [[600, 90, 'u']] },
    ],
  },
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

/** An empty layout passes. Otherwise every part has a place on the map, and the map places, wires and stacks copies for only parts that exist. */
export function checkLayout(partIds: string[], layout: MapLayout = SILO_MAP): void {
  const known = new Set(partIds);
  const where = 'website/src/workshop/map.ts';
  // An empty layout is no map at all: the page shows the tiles alone, and any plan passes.
  if (!hasMap(layout) && !layout.zones.length && !layout.notes.length && !layout.ghosts.length && !layout.wires.full.length && !layout.wires.fold.length) return;
  for (const id of partIds) {
    if (!layout.places[id]) throw new LayoutError(`The map has no place for the part "${id}" of plan/parts.json. Add it to ${where}.`);
  }
  for (const id of Object.keys(layout.places)) {
    if (!known.has(id)) throw new LayoutError(`${where} places the part "${id}", which plan/parts.json does not have.`);
  }
  for (const wire of [...layout.wires.full, ...layout.wires.fold]) {
    if (!known.has(wire.of)) throw new LayoutError(`${where} draws a wire for the part "${wire.of}", which plan/parts.json does not have.`);
  }
  for (const ghost of layout.ghosts) {
    if (!known.has(ghost.of)) throw new LayoutError(`${where} stacks a copy behind the part "${ghost.of}", which plan/parts.json does not have.`);
  }
}
