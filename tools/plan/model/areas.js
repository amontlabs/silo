// The areas' counts and states, as the Workshop's map shows them.

import { STARTER_STAGES } from './names.js';

/** @typedef {import('./status.js').Status} Status */
/** @typedef {{ built: number, building: number, planned: number, later: number, undecided: number, dropped: number }} Counts */
/** @typedef {'now' | 'done' | 'next' | 'planned' | 'open'} AreaState */
/** @typedef {{ id: string, status: Status, position: number, stage: string }} AreaRow */

/** @param {{ status: Status }[]} rows @returns {Counts} */
export function countRows(rows) {
  /** @type {Counts} */
  const counts = { built: 0, building: 0, planned: 0, later: 0, undecided: 0, dropped: 0 };
  for (const row of rows) counts[row.status] += 1;
  return counts;
}

/**
 * Each area's state, first match wins: now (a feature is building), done (something built, nothing
 * planned), next (the area holding the first planned feature in order among areas not being built;
 * at most one), planned (a feature is planned), open (nothing decided to build yet).
 * @param {{ rows: { status: Status, position: number }[], counts: Counts }[]} groups
 * @returns {AreaState[]}
 */
export function areaStates(groups) {
  /** @type {AreaState[]} */
  const states = groups.map(({ counts }) => (counts.building ? 'now' : counts.built && !counts.planned ? 'done' : counts.planned ? 'planned' : 'open'));
  let next = -1;
  let lowest = Infinity;
  groups.forEach(({ rows }, index) => {
    if (states[index] === 'now') return;
    for (const row of rows) {
      if (row.status === 'planned' && row.position < lowest) {
        lowest = row.position;
        next = index;
      }
    }
  });
  if (next !== -1) states[next] = 'next';
  return states;
}

/**
 * The areas with their features in order, counts and state.
 * @param {{ areas: { id: string, title: string, line: string }[], stages: { id: string, features: string[] }[], features: Record<string, { id: string, area?: string, stage: string, position: number }> }} plan
 * @param {(id: string) => Status} statusOf
 */
export function areaGroups(plan, statusOf) {
  const starter = new Set(plan.stages.slice(0, STARTER_STAGES).map((stage) => stage.id));
  const ordered = plan.stages.flatMap((stage) => stage.features).map((id) => plan.features[id]).filter(Boolean);
  const groups = plan.areas.map((area, index) => {
    /** @type {AreaRow[]} */
    const rows = ordered.filter((feature) => feature.area === area.id).map((feature) => ({ id: feature.id, status: statusOf(feature.id), position: feature.position, stage: feature.stage }));
    return { area, number: index + 1, rows, counts: countRows(rows), state: /** @type {AreaState} */ ('open'), first: rows.some((row) => starter.has(row.stage)) };
  });
  areaStates(groups).forEach((state, index) => { groups[index].state = state; });
  return groups;
}

function listNames(names) {
  return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** @param {{ area: { id: string, title: string }, state: AreaState, counts: Counts }[]} groups @returns {string} where the build is, in one line. */
export function statusLine(groups) {
  const now = groups.filter((group) => group.state === 'now').map((group) => group.area.title);
  const next = groups.find((group) => group.state === 'next');
  const built = groups.some((group) => group.counts.built > 0);
  if (now.length) return `Building ${listNames(now)}.${next ? ` ${next.area.title} is next.` : ''}`;
  if (!next) return '';
  if (built) return `${next.area.title} is next.`;
  return `Starting with ${next.area.title}.`;
}

/** @param {{ state: AreaState, counts: Counts }} group @returns {string} the few words on a part of the map. */
export function stateWords(group) {
  const { built, building, planned } = group.counts;
  switch (group.state) {
    case 'done': return 'done';
    case 'now': return `now · ${built}/${built + building + planned}`;
    case 'next': return 'next';
    case 'planned': return built ? `${built}/${built + planned} built` : `${planned} planned`;
    default: return '';
  }
}
