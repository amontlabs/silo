// Which paths under `plan/` are an entry's file or its notes. Pure: no `node:` import.

import { ID_PATTERN } from '../model/names.js';

const ID = ID_PATTERN.source.slice(1, -1);
const PART = '(?!decisions/|features/)[^/.][^/]*';

const DECISION = new RegExp(`^(?:(${PART})/)?decisions/(${ID})\\.(json|md)$`);
const FEATURE = new RegExp(`^(${PART})/features/(${ID})\\.(json|md)$`);

/**
 * What a path relative to `plan/` is: an entry's file (`decisions/<id>.json`,
 * `<part>/decisions/<id>.json`, `<part>/features/<id>.json`) or its notes (the `<id>.md` beside it).
 * Any other path is null.
 * @param {string} path
 * @returns {{ id: string, part: string | null, kind: 'decision' | 'feature', notes: boolean, dir: string } | null}
 */
export function entryPath(path) {
  const decision = DECISION.exec(path);
  const match = decision ?? FEATURE.exec(path);
  if (!match) return null;
  return { id: match[2], part: match[1] ?? null, kind: decision ? 'decision' : 'feature', notes: match[3] === 'md', dir: path.slice(0, path.lastIndexOf('/') + 1) };
}

/** The pathspecs (relative to the top of the repository) that hold every path `entryPath` accepts. */
export const PATHSPECS = Object.freeze([
  'plan/decisions/*.json', 'plan/*/decisions/*.json', 'plan/*/features/*.json',
  'plan/decisions/*.md', 'plan/*/decisions/*.md', 'plan/*/features/*.md',
].map((spec) => `:(top,glob)${spec}`));
