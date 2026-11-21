// The plan as the repository holds it, in plan/: read with tools/plan/read and checked with tools/plan/check
// at build time. Any error stops the build with the file, the field and the fix.
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { entryStatus, listKind, replacedBy, wordId } from '../../../tools/plan/model/index.js';
import { repoAddress } from '../../../tools/workshop/model/index.js';
import { entryHistory, readSteps } from '../../../tools/plan/history/index.js';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError, NO_MAP, pageLayout, SILO_MAP } from './map.ts';
import type { LayoutPlan, MapLayout } from './map.ts';
import { RESERVED_ADDRESSES } from './routes.ts';
import { playableFilms } from '../../scripts/films.mjs';
import type { BuildData, Change, Entry, EntryFacts, Facts, Film, Histories, Notes, Statement, Verdict } from './plan.ts';

export const PLAN_DIR = 'plan';

export class PlanFileError extends Error {}

interface RawEntry {
  id: string;
  kind: 'decision' | 'feature';
  part: string | null;
  stage: string;
  position: number;
  notes?: string;
  name?: string;
  what?: string;
  asks?: string;
  answer?: string;
  needs?: string[];
  verdict?: string;
  replaces?: string[];
  replaced_by?: string;
  started?: string;
  built?: string;
  suggest?: { verdict?: string; why?: string };
  sets?: Record<string, unknown>[];
  removes?: string[];
  adds?: string[];
  keeps?: string[];
  surfaces?: { app?: string; cli?: string };
}

interface RawPlan {
  parts: { id: string; title: string; line: string }[];
  stages: { id: string; title: string; line: string; entries: string[] }[];
  entries: Record<string, RawEntry>;
  words: { word: string; means: string; id: string | null }[];
  principles: { id: string; says: string; asks: string }[];
  properties: { id: string; says: string }[];
}

/** What the repository's history says of the plan's entries: each one's changes, newest first, and whether the clone it was read from is shallow. */
export interface HistorySource {
  changes: Map<string, Change[]>;
  shallow: boolean;
}

/** The map, and what the page may do beyond showing the plan: the history of its entries (none by default). */
export interface BuildOptions {
  map?: MapLayout;
  history?: HistorySource | null;
  /** The films whose page the build writes (scripts/films.mjs); none by default. */
  films?: string[];
  /** What each film of films/ says of itself and which of its entries it is behind (`readFilms`), in the order films/README.md gives; none by default. */
  filmInfo?: FilmInfo[];
}

/** A film as films/<id>/film.json describes it, with the ids of the entries it shows that changed after it. */
export type FilmInfo = Omit<Film, 'playable'>;

const hasNotes = (raw: RawEntry) => typeof raw.notes === 'string' && raw.notes.trim() !== '';

const ordered = (plan: RawPlan): RawEntry[] => plan.stages.flatMap((stage) => stage.entries).map((id) => plan.entries[id]).filter(Boolean);

/** The notes of the entries that have some, by id: the chunk the page loads when a person reaches "Notes". */
export function toNotes(plan: RawPlan): Notes {
  const notes: Notes = {};
  for (const raw of ordered(plan)) if (hasNotes(raw)) notes[raw.id] = raw.notes as string;
  return notes;
}

/** What a closed History line needs of an entry's changes (newest first): the newest and how many there are. */
function summaryOf(changes: Change[] | undefined): Entry['history'] {
  return changes?.length ? { date: changes[0].date, what: changes[0].what, count: changes.length } : null;
}

const list = (value: unknown): string[] => (Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []);
const text = (value: unknown): string => (typeof value === 'string' ? value : '');

/** A statement of a list, or an item of `sets`, as the page holds it; null for what is no statement. */
function statementOf(kind: Statement['kind'], item: Record<string, unknown>): Statement | null {
  if (kind === 'word') return typeof item.word === 'string' ? { id: wordId(item.word), kind, word: item.word, text: text(item.means), asks: null } : null;
  if (typeof item.id !== 'string') return null;
  return { id: item.id, kind, word: null, text: text(item.says), asks: kind === 'principle' ? text(item.asks) : null };
}

/** What the page draws of an entry from its file, as far as the file has it; null when it has none. */
function factsOf(raw: RawEntry): EntryFacts | null {
  const facts: EntryFacts = {};
  const suggest = raw.suggest;
  if (suggest && (suggest.verdict === 'take' || suggest.verdict === 'drop')) facts.suggest = { verdict: suggest.verdict, why: text(suggest.why) };
  const sets = (Array.isArray(raw.sets) ? raw.sets : []).flatMap((item) => {
    const kind = item && typeof item === 'object' ? listKind(item.in) : null;
    const statement = kind ? statementOf(kind, item) : null;
    return statement ? [statement] : [];
  });
  if (sets.length) facts.sets = sets;
  for (const key of ['removes', 'adds', 'keeps'] as const) if (list(raw[key]).length) facts[key] = list(raw[key]);
  const surfaces = raw.surfaces && typeof raw.surfaces === 'object' ? { ...(text(raw.surfaces.app) ? { app: text(raw.surfaces.app) } : {}), ...(text(raw.surfaces.cli) ? { cli: text(raw.surfaces.cli) } : {}) } : {};
  if (surfaces.app || surfaces.cli) facts.surfaces = surfaces;
  return Object.keys(facts).length ? facts : null;
}

/**
 * Whether an entry has anything to draw from its facts: a recommendation and what a yes would write only while it
 * has no verdict, what it added once taken, and a feature's properties and surfaces whatever its verdict.
 */
function hasFacts(raw: RawEntry): boolean {
  const facts = factsOf(raw);
  if (!facts) return false;
  const open = !raw.verdict;
  return Boolean((open && (facts.suggest || facts.sets || facts.removes)) || (raw.verdict === 'take' && facts.adds) || (raw.kind === 'feature' && (facts.keeps || facts.surfaces)));
}

/** Every statement in force, by id: the page's lists. */
function statementsOf(plan: RawPlan): Record<string, Statement> {
  const statements: Record<string, Statement> = {};
  for (const word of plan.words) {
    const statement = statementOf('word', word);
    if (statement) statements[statement.id] = statement;
  }
  for (const principle of plan.principles) {
    const statement = statementOf('principle', principle);
    if (statement) statements[statement.id] = statement;
  }
  for (const property of plan.properties) {
    const statement = statementOf('property', property);
    if (statement) statements[statement.id] = statement;
  }
  return statements;
}

/** The facts of the entries that have some, and the statements in force: the chunk the page loads when a row is first opened. */
export function toFacts(plan: RawPlan): Facts {
  const entries: Facts['entries'] = {};
  for (const raw of ordered(plan)) {
    const facts = hasFacts(raw) ? factsOf(raw) : null;
    if (facts) entries[raw.id] = facts;
  }
  return { entries, statements: statementsOf(plan) };
}

/** The changes of the entries of the plan, by id: the chunk the page loads when a History is first opened. */
export function toHistories(plan: RawPlan, source: HistorySource | null): Histories {
  const histories: Histories = {};
  for (const raw of ordered(plan)) {
    const changes = source?.changes.get(raw.id);
    if (changes?.length) histories[raw.id] = changes;
  }
  return histories;
}

/**
 * The parts each part depends on, by part id. Part X depends on part Y when an entry of X names an entry of Y in `needs`
 * and Y starts before X in the plan's order, so that no two parts depend on each other. A part starts at its first feature
 * in the order of order.json, or at its first entry when it has no feature. An entry of the same part, of no part, or that
 * a later part holds, says nothing. Each list follows `order`, the order of the map; a part the map does not place comes
 * after those it does, in the order of plan/parts.json.
 */
export function partDepends(parts: { id: string }[], entries: Pick<Entry, 'id' | 'kind' | 'part' | 'position' | 'needs'>[], order: string[]): Map<string, string[]> {
  const partOf = new Map(entries.map((entry) => [entry.id, entry.part]));
  const start = new Map<string, number>();
  for (const part of parts) {
    const own = entries.filter((entry) => entry.part === part.id);
    const features = own.filter((entry) => entry.kind === 'feature');
    const first = features.length ? features : own;
    if (first.length) start.set(part.id, Math.min(...first.map((entry) => entry.position)));
  }
  const rank = (id: string) => { const at = order.indexOf(id); return at === -1 ? order.length : at; };
  const sorted = parts.map((part) => part.id).sort((a, b) => rank(a) - rank(b));
  const depends = new Map(parts.map((part) => [part.id, new Set<string>()]));
  for (const entry of entries) {
    if (entry.part === null) continue;
    for (const id of entry.needs) {
      const other = partOf.get(id);
      if (other === undefined || other === null || other === entry.part) continue;
      if ((start.get(other) ?? Infinity) < (start.get(entry.part) ?? -Infinity)) depends.get(entry.part)?.add(other);
    }
  }
  return new Map(parts.map((part) => [part.id, sorted.filter((id) => depends.get(part.id)?.has(id))]));
}

/** The plan object of tools/plan/read, as the page holds it: decisions and features in the order of order.json. */
export function toBuildData(plan: RawPlan, { map = SILO_MAP, history = null, films = [], filmInfo = [] }: BuildOptions = {}): BuildData {
  const entries: Entry[] = ordered(plan).map((raw) => ({
    id: raw.id,
    kind: raw.kind,
    part: raw.part,
    stage: raw.stage,
    position: raw.position,
    name: (raw.kind === 'decision' ? raw.asks : raw.name) ?? '',
    what: (raw.kind === 'decision' ? raw.answer : raw.what) ?? null,
    needs: Array.isArray(raw.needs) ? raw.needs : [],
    verdict: (raw.verdict ?? null) as Verdict | null,
    replaces: Array.isArray(raw.replaces) ? raw.replaces : [],
    replacedBy: typeof raw.replaced_by === 'string' ? raw.replaced_by : null,
    started: raw.started ?? null,
    built: raw.built ?? null,
    notes: hasNotes(raw),
    more: hasFacts(raw),
    history: summaryOf(history?.changes.get(raw.id)),
  }));
  const depends = partDepends(plan.parts, entries, Object.keys(map.places));
  return {
    parts: plan.parts.map((part) => ({ id: part.id, title: part.title, line: part.line, dependsOn: depends.get(part.id)! })),
    stages: plan.stages.map((stage) => ({ id: stage.id, title: stage.title, line: stage.line })),
    entries,
    map: pageLayout(map, layoutPlanOf(plan)),
    shallow: history?.shallow === true,
    films: filmInfo.map((film) => ({ ...film, playable: films.includes(film.id) })),
  };
}

/** Checks the files of a plan, or throws with every problem. `exists` says whether a repository path exists. */
function checkedPlan(files: Map<string, string>, exists?: (path: string) => boolean): RawPlan {
  const { errors } = checkFiles(files, exists ? { exists } : {});
  if (errors.length) throw new PlanFileError(errors.map(formatProblem).join('\n'));
  return parsePlan(files).plan as unknown as RawPlan;
}

/** Checks the files of a plan and gives their notes, or throws with every problem. */
export function notesFrom(files: Map<string, string>): Notes {
  return toNotes(checkedPlan(files));
}

/** Checks the files of a plan and gives its histories, or throws with every problem. */
export function historiesFrom(files: Map<string, string>, source: HistorySource | null): Histories {
  return toHistories(checkedPlan(files), source);
}

/** Checks the files of a plan and gives its facts, or throws with every problem. */
export function factsFrom(files: Map<string, string>): Facts {
  return toFacts(checkedPlan(files));
}

/** What the map reads of a plan: its parts, the status of each decision and feature, and its words. */
function layoutPlanOf(plan: RawPlan): LayoutPlan {
  // The plan tool's model reads the entries as the files give them, which the check has already passed.
  const entries = plan.entries as unknown as Record<string, Parameters<typeof entryStatus>[0]>;
  const decisions: LayoutPlan['decisions'] = {};
  const features: LayoutPlan['features'] = {};
  for (const [id, entry] of Object.entries(entries)) {
    if (entry.kind === 'decision') decisions[id] = { status: entryStatus(entry, entries) as LayoutPlan['decisions'][string]['status'], replacedBy: replacedBy(entry) };
    else features[id] = { status: entryStatus(entry, entries) as LayoutPlan['features'][string]['status'] };
  }
  return { parts: plan.parts, decisions, features, words: plan.words.flatMap((word) => (word.id ? [word.id] : [])) };
}

/** Checks the files of a plan and gives what the map's check reads of it, or throws with every problem. */
export function layoutPlanFrom(files: Map<string, string>): LayoutPlan {
  return layoutPlanOf(checkedPlan(files));
}

/** Checks the files of a plan and gives the page's data, or throws with every problem. */
export function buildDataFrom(files: Map<string, string>, options: BuildOptions = {}, exists?: (path: string) => boolean): BuildData {
  const plan = checkedPlan(files, exists);
  const data = toBuildData(plan, options);
  for (const part of data.parts) {
    if ((RESERVED_ADDRESSES as readonly string[]).includes(part.id)) {
      throw new PlanFileError(`plan/parts.json: the part "${part.id}" takes an address the Workshop keeps for its own page (#${part.id}). Fix: give the part another id; the Workshop keeps ${RESERVED_ADDRESSES.join(', ')}.`);
    }
  }
  try {
    checkLayout(layoutPlanOf(plan), options.map ?? SILO_MAP);
  } catch (error) {
    if (error instanceof LayoutError) throw new PlanFileError(error.message);
    throw error;
  }
  return data;
}

/** The plan's folder: `WORKSHOP_PLAN` when set (a fixture plan, for a build that proves the page), else plan/ under `repository`. */
export function planDir(repository: string, env: Record<string, string | undefined> = process.env): string {
  return env.WORKSHOP_PLAN || join(repository, PLAN_DIR);
}

function planFilesIn(dir: string): Map<string, string> {
  try {
    return loadDir(dir);
  } catch {
    throw new PlanFileError(`${dir}: cannot be read`);
  }
}

/** What a build does to read the history of the repository: its git, and its way of fetching what a shallow clone lacks. */
export interface Git {
  readSteps: typeof readSteps;
  unshallow: (root: string) => void;
}

/** What a fetch of history may do: it never asks for a credential (the repository is public) and never waits more than five minutes. */
function fetchHistory(root: string, args: string[]): void {
  execFileSync('git', ['-C', root, ...args], { stdio: 'ignore', timeout: 5 * 60_000, env: { ...process.env, GIT_TERMINAL_PROMPT: '0' } });
}

/**
 * Fetches what a shallow clone lacks. First `git fetch --unshallow origin`; Vercel's clone has no remote, so when that
 * fails, the commit being built from the repository's public address, whatever branch it is on. Only objects are
 * fetched: the working tree and the checked-out commit stay as they are.
 */
export function unshallow(root: string, address = repoAddress()): void {
  try {
    fetchHistory(root, ['fetch', '--unshallow', 'origin']);
    return;
  } catch {
    // No such remote, or it cannot be reached: the public address is next.
  }
  const head = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8', timeout: 60_000 }).trim();
  fetchHistory(root, ['fetch', '--unshallow', address, head]);
}

/**
 * The changes of every entry, read from the git repository at `repository`. A shallow clone is fetched whole once
 * (from origin, else from the repository's public address) and read again; when both fail the build goes on with what
 * the clone holds and `shallow` says so.
 */
export function historyFromGit(repository: string, git: Git = { readSteps, unshallow }): HistorySource {
  let read = git.readSteps(repository);
  if (read.shallow) {
    try {
      git.unshallow(repository);
    } catch {
      // The host may not reach any remote: the build goes on with the clone it has.
    }
    read = git.readSteps(repository);
  }
  return { changes: entryHistory(read.steps) as Map<string, Change[]>, shallow: read.shallow };
}

const readOnce = new Map<string, HistorySource>();

/**
 * The history of this repository's plan, read once per build. A plan read from another folder (`WORKSHOP_PLAN`) has
 * none: its files are not this repository's.
 */
export function readHistory(repository: string): HistorySource | null {
  if (planDir(repository) !== join(repository, PLAN_DIR)) return null;
  let source = readOnce.get(repository);
  if (!source) readOnce.set(repository, (source = historyFromGit(repository)));
  return source;
}

/** The order films/README.md gives the films in: the ids of its table "The films, in the order they are watched". */
export function filmOrder(readme: string): string[] {
  const at = readme.indexOf('## The films, in the order they are watched');
  if (at === -1) return [];
  const ids: string[] = [];
  for (const line of readme.slice(at).split('\n').slice(1)) {
    if (line.startsWith('## ')) break;
    const id = /^\|\s*`([a-z0-9-]+)`\s*\|/.exec(line)?.[1];
    if (id) ids.push(id);
  }
  return ids;
}

type EntryFile = Record<string, unknown>;

/** The statements an entry sets or added: each id, with the text of its `sets` item, or null for an id of `adds` (its text is in the lists). */
function setOrAdded(entry: EntryFile): Map<string, string | null> {
  const statements = new Map<string, string | null>();
  for (const id of list(entry.adds)) statements.set(id, null);
  if (Array.isArray(entry.sets)) {
    for (const item of entry.sets as Record<string, unknown>[]) if (typeof item?.id === 'string') statements.set(item.id, JSON.stringify(item));
  }
  return statements;
}

/**
 * Whether an entry file, before and after a commit, changed in what the Workshop prints of it: its question or name,
 * its answer or what, the ids or the texts of the statements it sets or added, what it removes, what it keeps, its
 * surfaces; or it was dropped, replaced or deleted. A take alone changes none of these: it writes the ids of `sets` as
 * `adds` and deletes `sets` and `removes`, so the same statements stand, by id, and the texts are compared where both
 * sides hold them. A file that did not exist before is new, not changed.
 */
export function printedChanged(before: string | null, after: string | null): boolean {
  if (before === null) return false;
  if (after === null) return true;
  let was: EntryFile;
  let now: EntryFile;
  try {
    was = JSON.parse(before) as EntryFile;
    now = JSON.parse(after) as EntryFile;
  } catch {
    return false;
  }
  if (now.verdict === 'drop' && was.verdict !== 'drop') return true;
  if (typeof now.replaced_by === 'string' && was.replaced_by !== now.replaced_by) return true;
  const took = now.verdict === 'take' && was.verdict !== 'take';
  const printed = (entry: EntryFile) => JSON.stringify([
    entry.name ?? null, entry.asks ?? null, entry.what ?? null, entry.answer ?? null,
    took ? [] : [...list(entry.removes)].sort(), [...list(entry.keeps)].sort(), entry.surfaces ?? null,
  ]);
  if (printed(was) !== printed(now)) return true;
  const a = setOrAdded(was);
  const b = setOrAdded(now);
  if (a.size !== b.size || [...a.keys()].some((id) => !b.has(id))) return true;
  return [...a].some(([id, text]) => text !== null && b.get(id) !== null && b.get(id) !== text);
}

/** One change of an entry's file in a commit that is not a merge: its text before and after (null where the file did not exist). */
export interface FileChange {
  id: string;
  before: string | null;
  after: string | null;
}

/** The changes to the files of `ids` under plan/ in the commits that are not merges and come after `commit` (`commit..HEAD`), oldest last. */
export function entryFileChanges(repository: string, commit: string, ids: ReadonlySet<string>): FileChange[] {
  const git = (args: string[], input?: string) => execFileSync('git', ['-C', repository, ...args], { input, maxBuffer: 512 * 1024 * 1024, timeout: 120_000, stdio: ['pipe', 'pipe', 'ignore'] });
  try {
    const log = git(['log', '--no-merges', '--raw', '-z', '--no-abbrev', '--no-renames', '--format=%x01%H', `${commit}..HEAD`, '--', PLAN_DIR]).toString('utf8');
    const found: { id: string; old: string; now: string }[] = [];
    const tokens = log.split('\0');
    for (let i = 0; i < tokens.length; i += 1) {
      let token = tokens[i].replace(/^\n/, '');
      const head = token.indexOf('\x01');
      if (head !== -1) token = token.slice(head + 1).replace(/^[0-9a-f]+\s*/, '');
      if (!token.startsWith(':')) continue;
      const [, , old, now] = token.slice(1).split(' ');
      const path = tokens[(i += 1)] ?? '';
      const id = /(?:^|\/)(?:decisions|features)\/([^/]+)\.json$/.exec(path)?.[1];
      if (id && ids.has(id)) found.push({ id, old, now });
    }
    const none = (oid: string) => /^0+$/.test(oid);
    const blobs = new Map<string, string>();
    const oids = [...new Set(found.flatMap(({ old, now }) => [old, now]).filter((oid) => !none(oid)))];
    if (oids.length) {
      const out = git(['cat-file', '--batch'], oids.map((oid) => `${oid}\n`).join(''));
      let at = 0;
      for (const oid of oids) {
        const end = out.indexOf(10, at);
        if (end === -1) break;
        const [, type, size] = out.subarray(at, end).toString().split(' ');
        if (type === 'missing') {
          at = end + 1;
          continue;
        }
        blobs.set(oid, out.subarray(end + 1, end + 1 + Number(size)).toString('utf8'));
        at = end + 1 + Number(size) + 1;
      }
    }
    return found.map(({ id, old, now }) => ({ id, before: none(old) ? null : blobs.get(old) ?? null, after: none(now) ? null : blobs.get(now) ?? null }));
  } catch {
    return [];
  }
}

/** The last commit that is not a merge and touched a folder of the repository; null when git cannot say. */
function folderCommit(repository: string, folder: string): string | null {
  try {
    return execFileSync('git', ['-C', repository, 'log', '-1', '--no-merges', '--format=%H', '--', folder], { encoding: 'utf8', timeout: 60_000, stdio: ['ignore', 'pipe', 'ignore'] }).trim() || null;
  } catch {
    return null;
  }
}

/**
 * The films of `repository`: each real folder directly under films/ that holds a film.json, in the order films/README.md
 * gives (one it does not name comes after, by id). A film is behind on an entry it shows when a commit that is not a
 * merge and comes after the film's last one (in the order of the commits, not their times) changed what the Workshop
 * prints of that entry (`printedChanged`). A shallow history says nothing is behind.
 */
export function readFilms(
  repository: string,
  shallow: boolean,
  lastCommit: (folder: string) => string | null = (folder) => folderCommit(repository, folder),
  changesAfter: (commit: string, ids: ReadonlySet<string>) => FileChange[] = (commit, ids) => entryFileChanges(repository, commit, ids),
): FilmInfo[] {
  const dir = join(repository, 'films');
  if (!existsSync(dir)) return [];
  const films: FilmInfo[] = [];
  for (const name of readdirSync(dir).sort()) {
    const file = join(dir, name, 'film.json');
    if (name.startsWith('.') || !lstatSync(join(dir, name)).isDirectory() || !existsSync(file)) continue;
    const raw = JSON.parse(readFileSync(file, 'utf8')) as { title?: unknown; line?: unknown; shows?: unknown };
    const shows = list(raw.shows);
    const commit = shallow ? null : lastCommit(`films/${name}`);
    const changes = commit ? changesAfter(commit, new Set(shows)) : [];
    const behind = shows.filter((id) => changes.some((change) => change.id === id && printedChanged(change.before, change.after)));
    films.push({ id: name, title: typeof raw.title === 'string' ? raw.title : name, line: typeof raw.line === 'string' ? raw.line : '', shows, behind });
  }
  const readme = join(dir, 'README.md');
  const order = existsSync(readme) ? filmOrder(readFileSync(readme, 'utf8')) : [];
  const rank = (id: string) => (order.includes(id) ? order.indexOf(id) : order.length);
  return films.sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id));
}

/** Reads the plan of `repository` and returns the page's data for the real build. Silo's map draws plan/; another plan has none. */
export function readBuildData(repository: string, map: MapLayout = planDir(repository) === join(repository, PLAN_DIR) ? SILO_MAP : NO_MAP): BuildData {
  const films = playableFilms(join(repository, 'films'));
  const history = readHistory(repository);
  // Another plan's folder is not this repository's: its films are not shown with it.
  const filmInfo = history ? readFilms(repository, history.shallow) : [];
  return buildDataFrom(planFilesIn(planDir(repository)), { map, history, films, filmInfo }, (path) => existsSync(join(repository, path)));
}

/** Reads the plan of `repository` and returns its facts, or throws. */
export function readFacts(repository: string): Facts {
  return factsFrom(planFilesIn(planDir(repository)));
}

/** Reads the plan of `repository` and returns its notes, or throws. */
export function readNotes(repository: string): Notes {
  return notesFrom(planFilesIn(planDir(repository)));
}

/** Reads the plan of `repository` and returns the changes of its entries, or throws. */
export function readHistories(repository: string): Histories {
  return historiesFrom(planFilesIn(planDir(repository)), readHistory(repository));
}
