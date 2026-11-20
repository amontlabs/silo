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

/** Silo's map. It is not drawn yet: with no place, the page shows the parts as tiles only. */
export const SILO_MAP: MapLayout = {
  places: {},
  zones: [],
  notes: [],
  ghosts: [],
  wires: { full: [], fold: [] },
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
