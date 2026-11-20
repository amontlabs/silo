// The plan as the repository holds it, in plan/: read with tools/plan/read and checked with tools/plan/check
// at build time. Any error stops the build with the file, the field and the fix.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { listKind, wordId } from '../../../tools/plan/model/index.js';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError, SILO_MAP } from './map.ts';
import type { MapLayout } from './map.ts';
import { RESERVED_ADDRESSES } from './routes.ts';
import type { BuildData, Entry, EntryFacts, Facts, Notes, Statement, Verdict } from './plan.ts';

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

/** The map, and what the page may do beyond showing the plan: entries' threads (off by default). */
export interface BuildOptions {
  map?: MapLayout;
  threads?: boolean;
}

const hasNotes = (raw: RawEntry) => typeof raw.notes === 'string' && raw.notes.trim() !== '';

const ordered = (plan: RawPlan): RawEntry[] => plan.stages.flatMap((stage) => stage.entries).map((id) => plan.entries[id]).filter(Boolean);

/** The notes of the entries that have some, by id: the chunk the page loads when a person reaches "Notes". */
export function toNotes(plan: RawPlan): Notes {
  const notes: Notes = {};
  for (const raw of ordered(plan)) if (hasNotes(raw)) notes[raw.id] = raw.notes as string;
  return notes;
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

/** The plan object of tools/plan/read, as the page holds it: decisions and features in the order of order.json. */
export function toBuildData(plan: RawPlan, { map = SILO_MAP, threads = false }: BuildOptions = {}): BuildData {
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
    built: raw.built ?? null,
    notes: hasNotes(raw),
    more: hasFacts(raw),
  }));
  return {
    parts: plan.parts.map((part) => ({ id: part.id, title: part.title, line: part.line })),
    stages: plan.stages.map((stage) => ({ id: stage.id, title: stage.title, line: stage.line })),
    entries,
    map,
    threads,
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

/** Checks the files of a plan and gives its facts, or throws with every problem. */
export function factsFrom(files: Map<string, string>): Facts {
  return toFacts(checkedPlan(files));
}

/** Checks the files of a plan and gives the page's data, or throws with every problem. */
export function buildDataFrom(files: Map<string, string>, options: BuildOptions = {}, exists?: (path: string) => boolean): BuildData {
  const data = toBuildData(checkedPlan(files, exists), options);
  for (const part of data.parts) {
    if ((RESERVED_ADDRESSES as readonly string[]).includes(part.id)) {
      throw new PlanFileError(`plan/parts.json: the part "${part.id}" takes an address the Workshop keeps for its own page (#${part.id}). Fix: give the part another id; the Workshop keeps ${RESERVED_ADDRESSES.join(', ')}.`);
    }
  }
  try {
    checkLayout(data.parts.map((part) => part.id), data.map);
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

/** Reads the plan of `repository` and returns the page's data for the real build: no threads. */
export function readBuildData(repository: string, map: MapLayout = SILO_MAP): BuildData {
  return buildDataFrom(planFilesIn(planDir(repository)), { map }, (path) => existsSync(join(repository, path)));
}

/** Reads the plan of `repository` and returns its facts, or throws. */
export function readFacts(repository: string): Facts {
  return factsFrom(planFilesIn(planDir(repository)));
}

/** Reads the plan of `repository` and returns its notes, or throws. */
export function readNotes(repository: string): Notes {
  return notesFrom(planFilesIn(planDir(repository)));
}
