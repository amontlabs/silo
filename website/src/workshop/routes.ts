// Addresses: `#<part>` and `#<part>/<entry id>`, `#product` (the decisions of the whole product) and `#suggested`, each
// with an optional open row, and `#films/<film id>`, a film's place. In All (`?view=all`, all.ts) the same `#<part>/<entry
// id>` opens the row in the list. The older `#latest` and `#left` open All in the order Latest and with the dropped
// entries; `#entry-<id>` and `#issue-<n>` still open that row where it now lives.
import type { Query } from './all.ts';
import type { Plan, Row } from './plan.ts';

export type Place =
  | { kind: 'home' }
  | { kind: 'part'; part: string }
  | { kind: 'film'; film: string }
  | { kind: 'suggested' }
  | { kind: 'product' };

export const HOME: Place = { kind: 'home' };

/** The addresses the page keeps for its own places, and the old ones it still reads: a part may not take one of these ids. */
export const RESERVED_ADDRESSES = ['product', 'left', 'suggested', 'latest', 'films'] as const;

export function samePlace(a: Place, b: Place): boolean {
  if (a.kind === 'part') return b.kind === 'part' && a.part === b.part;
  if (a.kind === 'film') return b.kind === 'film' && a.film === b.film;
  return a.kind === b.kind;
}

/** Where a row is shown. */
export function placeOf(row: Row): Place {
  if (row.kind === 'suggestion') return { kind: 'suggested' };
  return row.part ? { kind: 'part', part: row.part } : { kind: 'product' };
}

/** Where a row opens from a place: in that place when it is shown there (a dropped entry also sits in the left-out list), else where it lives. */
export function placeFrom(row: Row, place: Place, plan: Plan): Place {
  return rowsOf(plan, place).some((item) => item.key === row.key) ? place : placeOf(row);
}

/** The rows a place shows, in order. */
export function rowsOf(plan: Plan, place: Place): Row[] {
  switch (place.kind) {
    case 'part': return plan.parts.find((group) => group.part.id === place.part)?.rows ?? [];
    case 'suggested': return plan.suggestions;
    case 'product': return plan.product;
    case 'film': {
      const film = plan.films.find((one) => one.id === place.film);
      const rows = new Map([...plan.parts.flatMap((group) => group.rows), ...plan.product].map((row) => [row.key, row]));
      return film ? film.rows.map((key) => rows.get(key)).filter((row): row is Row => row !== undefined) : [];
    }
    default: return [];
  }
}

const rowPart = (key: string) => (key.startsWith('entry-') ? key.slice('entry-'.length) : key);

/** The row an address's tail names: an entry of the plan by its id first, whatever the id looks like, then a suggestion by its `issue-<n>` key. */
const rowNamed = (rows: Map<string, Row>, tail: string): Row | undefined =>
  rows.get(`entry-${tail}`) ?? (/^issue-/.test(tail) ? rows.get(tail) ?? featureOf(rows, tail) : undefined);

/** The entry whose thread is the issue `issue-<n>` names: a suggestion that became a feature shows as the feature. */
function featureOf(rows: Map<string, Row>, key: string): Row | undefined {
  const n = /^issue-(\d+)$/.exec(key)?.[1];
  if (n === undefined) return undefined;
  for (const row of rows.values()) if (row.kind === 'entry' && row.issue === Number(n)) return row;
  return undefined;
}

export function formatHash(place: Place, key: string | null = null): string {
  const tail = key ? `/${rowPart(key)}` : '';
  switch (place.kind) {
    case 'part': return `#${place.part}${tail}`;
    case 'suggested': return `#suggested${tail}`;
    case 'product': return `#product${tail}`;
    case 'film': return `#films/${place.film}${tail}`;
    default: return '';
  }
}

export function decodeHash(hash: string): string | null {
  try {
    return decodeURIComponent(hash.replace(/^#/, '')) || null;
  } catch {
    return null;
  }
}

/** What an address names: a place, the row open in it, and, for an old address that was a place, what it asks of All. */
export interface Resolved {
  place: Place;
  key: string | null;
  all?: Partial<Query>;
}

/** The address of a row where it lives. */
export const rowHash = (row: Row): string => formatHash(placeOf(row), row.key);

/**
 * The place and open row an address names, or null when it names nothing on this plan (yet: a suggestion
 * appears only once the live state arrives). A row named under the wrong place opens where it lives.
 */
export function resolveHash(hash: string, plan: Plan, rows: Map<string, Row>): Resolved | null {
  if (!hash.replace(/^#/, '')) return { place: HOME, key: null };
  const raw = decodeHash(hash);
  if (raw === null) return null;
  const [head, ...rest] = raw.split('/');
  const tail = rest.join('/');
  // An entry is found by its id whatever place the address names; the address is then rewritten to where it is.
  const named = (place: Place) => {
    if (!tail) return { place, key: null };
    const row = rowNamed(rows, tail);
    if (row) return { place: placeFrom(row, place, plan), key: row.key };
    // Entries come from the plan file: one that is not there no longer exists, and its part opens.
    // Issues may only appear with the live state, so those wait.
    return place.kind === 'part' && !/^issue-/.test(tail) ? { place, key: null } : null;
  };
  // Latest and Left out were places: they are All, in the order Latest and filtered to the dropped entries.
  if (head === 'latest' && !tail) return { place: HOME, key: null, all: { order: 'latest' } };
  if (head === 'left' && !tail) return { place: HOME, key: null, all: { states: ['dropped'] } };
  if (head === 'left' || head === 'latest') {
    const row = rowNamed(rows, tail);
    return row ? { place: placeOf(row), key: row.key } : null;
  }
  if (head === 'films') {
    const [id, ...more] = rest;
    if (!plan.films.some((film) => film.id === id)) return null;
    const place: Place = { kind: 'film', film: id };
    if (!more.length) return { place, key: null };
    const row = rowNamed(rows, more.join('/'));
    return row ? { place: placeFrom(row, place, plan), key: row.key } : { place, key: null };
  }
  if (head === 'suggested' || head === 'product') return named({ kind: head });
  if (plan.parts.some((group) => group.part.id === head)) return named({ kind: 'part', part: head });
  const loose = tail ? rowNamed(rows, tail) : rows.get(head) ?? (/^issue-/.test(head) ? featureOf(rows, head) : undefined);
  if (loose && (tail || /^(entry|issue)-/.test(head))) return { place: placeOf(loose), key: loose.key };
  return null;
}
