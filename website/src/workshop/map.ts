// The drawing of Silo: where each area sits on the full map and on the folded one, the zones around them
// and the wires between them. Units are 1/1136 of the map's width; the full map is 572 units high, the fold 180.
// The build checks this table against docs/product/areas.json (`checkLayout`).

export type Box = [x: number, y: number, w: number, h: number];

export interface Place {
  full: Box;
  fold: Box;
  /** Drawn inside another part: smaller title, above its parent. */
  inner?: boolean;
  /** Tall enough that its folded title sits at the top, not in the middle. */
  tall?: boolean;
  /** Title and state line offsets for short boxes. */
  ty?: number;
  sy?: number;
}

export const MAP_WIDTH = 1136;
export const MAP_HEIGHT = 572;
export const FOLD_HEIGHT = 180;

/** In Tab order: roughly left to right, then top to bottom. */
export const LAYOUT: Record<string, Place> = {
  app: { full: [48, 114, 192, 144], fold: [40, 38, 176, 30] },
  releases: { full: [48, 322, 192, 56], fold: [40, 84, 176, 30], ty: 9, sy: 31 },
  engine: { full: [284, 114, 112, 360], fold: [248, 38, 112, 114], tall: true },
  trust: { full: [292, 414, 96, 52], fold: [254, 122, 100, 24], inner: true, ty: 7, sy: 29 },
  computers: { full: [456, 114, 328, 128], fold: [412, 34, 388, 38] },
  agents: { full: [472, 174, 108, 52], fold: [520, 41, 104, 24], inner: true, ty: 7, sy: 29 },
  setup: { full: [584, 174, 96, 52], fold: [628, 41, 88, 24], inner: true, ty: 7, sy: 29 },
  desktop: { full: [684, 174, 84, 52], fold: [720, 41, 72, 24], inner: true, ty: 7, sy: 29 },
  checkpoints: { full: [556, 242, 128, 48], fold: [548, 72, 104, 24], ty: 6, sy: 27 },
  forks: { full: [456, 322, 152, 64], fold: [412, 84, 92, 30] },
  changes: { full: [652, 326, 132, 56], fold: [668, 84, 132, 30], ty: 9, sy: 31 },
  macos: { full: [456, 430, 196, 56], fold: [412, 122, 164, 30], ty: 9, sy: 31 },
  sharing: { full: [664, 430, 120, 56], fold: [584, 122, 92, 30], ty: 9, sy: 31 },
  access: { full: [812, 154, 104, 48], fold: [824, 38, 96, 30], ty: 6, sy: 27 },
  tasks: { full: [936, 18, 176, 48], fold: [944, 10, 168, 30], ty: 6, sy: 27 },
  devices: { full: [936, 426, 176, 128], fold: [944, 140, 168, 30] },
};

export interface Zone {
  label: string;
  full: Box;
  fold: Box;
  corners?: boolean;
  rounded?: boolean;
}

export const ZONES: Zone[] = [
  { label: 'YOUR MAC', full: [24, 18, 840, 536], fold: [16, 10, 856, 160], corners: true },
  { label: 'VIRTUAL MACHINES', full: [440, 90, 360, 416], fold: [400, 22, 412, 138], rounded: true },
];

/** Text that is not a part: what Access reaches. */
export const OUTSIDE = { title: 'GitHub', line: 'the internet', full: [946, 160, 150, 40] as Box, fold: [952, 44, 150, 36] as Box };

/** The copies stacked behind Forks. */
export const GHOSTS: { of: string; full: Box; fold: Box }[] = [
  { of: 'forks', full: [468, 334, 152, 64], fold: [412, 84, 92, 30] },
  { of: 'forks', full: [462, 328, 152, 64], fold: [412, 84, 92, 30] },
];

export type Arrow = [x: number, y: number, dir: 'r' | 'l' | 'u' | 'd'];

export interface Wire {
  /** The area whose state colours the wire. */
  of: string;
  d: string;
  arrows?: Arrow[];
  label?: [x: number, y: number, text: string, anchor?: 'middle'];
  /** A cross at the end: the copy that is dropped. */
  cross?: [x: number, y: number];
}

export const WIRES_FULL: Wire[] = [
  { of: 'app', d: 'M240 178H277', arrows: [[284, 178, 'r']] },
  { of: 'computers', d: 'M396 178H449', arrows: [[456, 178, 'r']], label: [420, 170, 'runs', 'middle'] },
  { of: 'releases', d: 'M144 322V265', arrows: [[144, 258, 'u']], label: [153, 295, 'updates'] },
  { of: 'tasks', d: 'M936 42H340V107', arrows: [[340, 114, 'd']] },
  { of: 'devices', d: 'M340 474V530H929', arrows: [[936, 530, 'r']] },
  { of: 'access', d: 'M784 178H812M916 178H932M936 163V197' },
  { of: 'forks', d: 'M508 242V315', arrows: [[508, 322, 'd']], label: [517, 287, 'fork'] },
  { of: 'changes', d: 'M622 354H645', arrows: [[652, 354, 'r']] },
  { of: 'changes', d: 'M718 326V249', arrows: [[718, 242, 'u']], label: [727, 287, 'merge'] },
  { of: 'changes', d: 'M718 382V399', cross: [718, 407], label: [730, 411, 'drop'] },
];

export const WIRES_FOLD: Wire[] = [
  { of: 'app', d: 'M216 53H241', arrows: [[248, 53, 'r']] },
  { of: 'computers', d: 'M360 53H405', arrows: [[412, 53, 'r']] },
  { of: 'releases', d: 'M128 84V75', arrows: [[128, 68, 'u']] },
  { of: 'access', d: 'M800 53H824M920 53H938M942 44V62' },
  { of: 'forks', d: 'M458 72V77', arrows: [[458, 84, 'd']] },
  { of: 'changes', d: 'M504 105H661', arrows: [[668, 105, 'r']] },
  { of: 'changes', d: 'M734 84V79', arrows: [[734, 72, 'u']] },
];

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

export class LayoutError extends Error {}

/** Every area has a place on the map, and the map places, and wires, only areas that exist. */
export function checkLayout(areaIds: string[], layout = LAYOUT, wires: Wire[] = [...WIRES_FULL, ...WIRES_FOLD, ...GHOSTS.map((g) => ({ of: g.of, d: '' }))]): void {
  const known = new Set(areaIds);
  const where = 'website/src/workshop/map.ts';
  for (const id of areaIds) {
    if (!layout[id]) throw new LayoutError(`The map has no place for the area "${id}" of docs/product/areas.json. Add it to ${where}.`);
  }
  for (const id of Object.keys(layout)) {
    if (!known.has(id)) throw new LayoutError(`${where} places the area "${id}", which docs/product/areas.json does not have.`);
  }
  for (const wire of wires) {
    if (!known.has(wire.of)) throw new LayoutError(`${where} draws a wire for the area "${wire.of}", which docs/product/areas.json does not have.`);
  }
}
