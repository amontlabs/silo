// The plan as the repository holds it, in plan/: read with tools/plan/read and checked with tools/plan/check
// at build time. Any error stops the build with the file, the field and the fix.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { entryStatus, listKind, replacedBy, wordId } from '../../../tools/plan/model/index.js';
import { repoAddress } from '../../../tools/workshop/model/index.js';
import { entryHistory, readSteps } from '../../../tools/plan/history/index.js';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError, NO_MAP, pageLayout, SILO_MAP } from './map.ts';
import type { LayoutPlan, MapLayout } from './map.ts';
import { RESERVED_ADDRESSES } from './routes.ts';
import type { BuildData, Change, Entry, EntryFacts, Facts, Histories, Notes, Statement, Verdict } from './plan.ts';

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
}

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
 * What each part needs and what needs it, by part id. A part needs another when one of its entries names an entry of
 * that one in `needs`; an entry of the same part or of no part says nothing. Both lists follow `order`, the order of
 * the map; a part the map does not place comes after those it does, in the order of plan/parts.json.
 */
export function partNeeds(parts: { id: string }[], entries: Pick<Entry, 'id' | 'part' | 'needs'>[], order: string[]): Map<string, { needs: string[]; neededBy: string[] }> {
  const partOf = new Map(entries.map((entry) => [entry.id, entry.part]));
  const rank = (id: string) => { const at = order.indexOf(id); return at === -1 ? order.length : at; };
  const sorted = parts.map((part) => part.id).sort((a, b) => rank(a) - rank(b));
  const needs = new Map(parts.map((part) => [part.id, new Set<string>()]));
  const neededBy = new Map(parts.map((part) => [part.id, new Set<string>()]));
  for (const entry of entries) {
    if (entry.part === null) continue;
    for (const id of entry.needs) {
      const other = partOf.get(id);
      if (other === undefined || other === null || other === entry.part) continue;
      needs.get(entry.part)?.add(other);
      neededBy.get(other)?.add(entry.part);
    }
  }
  return new Map(parts.map((part) => [part.id, { needs: sorted.filter((id) => needs.get(part.id)?.has(id)), neededBy: sorted.filter((id) => neededBy.get(part.id)?.has(id)) }]));
}

/** The plan object of tools/plan/read, as the page holds it: decisions and features in the order of order.json. */
export function toBuildData(plan: RawPlan, { map = SILO_MAP, history = null }: BuildOptions = {}): BuildData {
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
  const related = partNeeds(plan.parts, entries, Object.keys(map.places));
  return {
    parts: plan.parts.map((part) => ({ id: part.id, title: part.title, line: part.line, ...related.get(part.id)! })),
    stages: plan.stages.map((stage) => ({ id: stage.id, title: stage.title, line: stage.line })),
    entries,
    map: pageLayout(map, layoutPlanOf(plan)),
    shallow: history?.shallow === true,
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

/** Reads the plan of `repository` and returns the page's data for the real build. Silo's map draws plan/; another plan has none. */
export function readBuildData(repository: string, map: MapLayout = planDir(repository) === join(repository, PLAN_DIR) ? SILO_MAP : NO_MAP): BuildData {
  return buildDataFrom(planFilesIn(planDir(repository)), { map, history: readHistory(repository) }, (path) => existsSync(join(repository, path)));
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
