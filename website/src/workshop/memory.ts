// What the browser remembers between visits, so the first frame already shows them: the view and the order used last, the last live state the page
// showed (its own confirmed writes applied) and who was signed in. Without storage the page waits for the server.
import { hasSessionHint } from './api';
import type { Me } from './api';
import type { LiveState } from './plan';

const STATE = 'silo-workshop-state';
const PERSON = 'silo-workshop-me';
/** A remembered state older than this is not shown. */
export const MEMORY_MS = 7 * 24 * 3600_000;
/** Off in fixture mode unless asked for, so a fixture's delay still shows a first visit. */
export const memoryConfig = { on: true };

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);

function isIssue(value: unknown): boolean {
  return isObject(value) && (value.kind === 'entry' || value.kind === 'suggestion') && typeof value.title === 'string'
    && (value.state === 'open' || value.state === 'closed') && typeof value.up === 'number' && typeof value.comments === 'number'
    && (value.entry === null || typeof value.entry === 'string');
}

/** A state with the shape the page reads, whatever wrote it. One without `threads`, from before entries had them, is not shown. */
export function isState(value: unknown): value is LiveState {
  return isObject(value) && typeof value.repo === 'string' && typeof value.fetchedAt === 'string' && Number.isFinite(Date.parse(value.fetchedAt))
    && isObject(value.issues) && Object.values(value.issues).every(isIssue)
    && isObject(value.threads) && Object.values(value.threads).every((n) => typeof n === 'number' && Number.isSafeInteger(n));
}

function read(key: string): unknown {
  // At build time there is no browser and nothing is remembered.
  if (!memoryConfig.on || typeof window === 'undefined') return null;
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? null : (JSON.parse(raw) as unknown);
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  if (!memoryConfig.on) return;
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Without storage the next visit waits for the server, as a first one does.
  }
}

/** The state shown last, unless it is over seven days old or not what the page expects. */
export function recallState(now: number): LiveState | null {
  const state = read(STATE);
  if (!isState(state) || now - Date.parse(state.fetchedAt) > MEMORY_MS) return null;
  // A state an older page stored may hold the list of the latest comments, which this page does not read.
  const { old: _old, latest: _latest, ...rest } = state as LiveState & { latest?: unknown };
  return rest;
}

export function rememberState(state: LiveState) {
  const { old: _old, ...rest } = state;
  write(STATE, rest);
}

/** The person signed in last, while the session marker says a session may still be there. */
export function recallPerson(): Me | null {
  if (!hasSessionHint()) return null;
  const person = read(PERSON);
  if (!isObject(person) || typeof person.login !== 'string' || !person.login || typeof person.avatar !== 'string') return null;
  return { login: person.login, avatar: person.avatar };
}

export function rememberPerson(person: Me | null) {
  write(PERSON, person ? { login: person.login, avatar: person.avatar } : null);
}

const VIEW = 'silo-workshop-view';

/** The view and All's order a person used last, so a plain address shows them again. */
export interface Habit {
  view: 'map' | 'all';
  order: 'build' | 'latest';
}

export function recallHabit(): Habit | null {
  const habit = read(VIEW);
  if (!isObject(habit) || (habit.view !== 'map' && habit.view !== 'all') || (habit.order !== 'build' && habit.order !== 'latest')) return null;
  return { view: habit.view, order: habit.order };
}

export function rememberHabit(habit: Habit) {
  write(VIEW, habit);
}
