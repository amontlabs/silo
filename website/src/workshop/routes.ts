// Addresses: `#<area>` and `#<area>/<entry id>`, `#suggested`, `#left` and `#decisions`, each with an optional
// open row. The older `#entry-<id>`, `#issue-<n>` and `#decided-<i>` still open that row where it now lives.
import type { Plan, Row } from './plan';

export type Place =
  | { kind: 'home' }
  | { kind: 'area'; area: string }
  | { kind: 'suggested' }
  | { kind: 'left' }
  | { kind: 'decisions' };

export const HOME: Place = { kind: 'home' };

export function samePlace(a: Place, b: Place): boolean {
  return a.kind === b.kind && (a.kind !== 'area' || (b.kind === 'area' && a.area === b.area));
}

/** Where a row is shown. */
export function placeOf(row: Row): Place {
  if (row.kind === 'suggestion') return { kind: 'suggested' };
  if (row.kind === 'decision' || row.kind === 'decided') return { kind: 'decisions' };
  return row.area ? { kind: 'area', area: row.area } : { kind: 'left' };
}

/** The rows a place shows, in order. */
export function rowsOf(plan: Plan, place: Place): Row[] {
  switch (place.kind) {
    case 'area': return plan.areas.find((group) => group.area.id === place.area)?.rows ?? [];
    case 'suggested': return plan.suggestions;
    case 'left': return plan.left;
    case 'decisions': return [...plan.openDecisions, ...plan.decided];
    default: return [];
  }
}

const rowPart = (key: string) => (key.startsWith('entry-') ? key.slice('entry-'.length) : key);
const rowKey = (part: string) => (/^(issue|decided)-/.test(part) ? part : `entry-${part}`);

export function formatHash(place: Place, key: string | null = null): string {
  const tail = key ? `/${rowPart(key)}` : '';
  switch (place.kind) {
    case 'area': return `#${place.area}${tail}`;
    case 'suggested': return `#suggested${tail}`;
    case 'left': return `#left${tail}`;
    case 'decisions': return `#decisions${tail}`;
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

/**
 * The place and open row an address names, or null when it names nothing on this plan (yet: a suggestion
 * appears only once the live state arrives). A row named under the wrong place opens where it lives.
 */
export function resolveHash(hash: string, plan: Plan, rows: Map<string, Row>): { place: Place; key: string | null } | null {
  if (!hash.replace(/^#/, '')) return { place: HOME, key: null };
  const raw = decodeHash(hash);
  if (raw === null) return null;
  const [head, ...rest] = raw.split('/');
  const tail = rest.join('/');
  const named = (place: Place) => {
    if (!tail) return { place, key: null };
    const row = rows.get(rowKey(tail));
    return row ? { place: placeOf(row), key: row.key } : null;
  };
  if (head === 'suggested' || head === 'left' || head === 'decisions') return named({ kind: head });
  if (plan.areas.some((group) => group.area.id === head)) return named({ kind: 'area', area: head });
  if (!tail && /^(entry|issue|decided)-/.test(head)) {
    const row = rows.get(head);
    return row ? { place: placeOf(row), key: row.key } : null;
  }
  return null;
}
