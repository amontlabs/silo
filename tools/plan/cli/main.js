// `plan`: the commands. Exit codes: 0 done, 1 refused or errors found, 2 wrong use.
// Nothing here calls the network: the command reads `plan/`, checks it, and commits a verdict.

import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { checkChange, checkFiles, formatProblem } from '../check/index.js';
import { entryStatus, waitingFor } from '../model/status.js';
import { PlanError, parsePlan } from '../read/index.js';
import { drop, take } from '../write/index.js';
import { Refused, UsageError, parseArgs } from './args.js';
import { baseFiles, existsIn, findRoot, git, planFiles, ruledByOwner, uncommitted } from './repo.js';

export const COMMANDS = [
  { name: '', usage: 'plan', what: 'What is next: the first entry with no verdict, the features ready to build, the features that wait and for what.' },
  { name: 'show', usage: 'plan show <id>', what: 'One entry or one sentence, with what it links to and what binds it.' },
  { name: 'take', usage: 'plan take <id>', what: "The owner's yes: writes what the entry sets into the lists and commits \"Owner: take <id>\"." },
  { name: 'drop', usage: 'plan drop <id>', what: "The owner's no: commits \"Owner: drop <id>\"." },
  { name: 'check', usage: 'plan check', what: 'The plan holds together. The merge gate and the site\'s build run it.' },
];

/** @typedef {{ cwd: string, out: (text: string) => void, err: (text: string) => void }} Context */

/** @param {string[]} argv @param {Partial<Context>} [context] @returns {Promise<number>} the exit code */
export async function main(argv, context = {}) {
  /** @type {Context} */
  const ctx = {
    cwd: process.cwd(),
    out: (text) => process.stdout.write(`${text}\n`),
    err: (text) => process.stderr.write(`${text}\n`),
    ...context,
  };
  const json = argv.includes('--json');
  try {
    if (argv.includes('--help') || argv.includes('-h')) return help(ctx, argv);
    // --json may come before or after the command name; the command always sees it after.
    const [command, ...others] = argv.filter((arg) => arg !== '--json');
    const rest = json ? [...others, '--json'] : others;
    switch (command) {
      case undefined: return next(ctx, rest);
      case 'show': return show(ctx, rest);
      case 'take': return verdict(ctx, rest, 'take');
      case 'drop': return verdict(ctx, rest, 'drop');
      case 'check': return check(ctx, rest);
      default: throw new UsageError(`unknown command "${command}"; plan --help lists the commands`);
    }
  } catch (error) {
    const code = error instanceof UsageError ? 2 : 1;
    const message = String(/** @type {any} */ (error)?.message ?? error);
    const problems = error instanceof PlanError || error instanceof Refused ? /** @type {any} */ (error).problems : undefined;
    if (json) ctx.out(JSON.stringify({ ok: false, error: message, ...(problems ? { errors: problems } : {}) }, null, 2));
    else if (error instanceof PlanError) for (const problem of error.problems) ctx.err(formatProblem(problem));
    else {
      ctx.err(`plan: ${message.split('\n')[0]}`);
      for (const problem of problems ?? []) ctx.err(formatProblem(problem));
    }
    return code;
  }
}

/** A file's text, or null when it cannot be read. @param {string} path */
function readOr(path) {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return null;
  }
}

/** A refusal that carries the check's problems. @param {string} message @param {any[]} [problems] */
function refusal(message, problems) {
  const error = new Refused(message);
  if (problems) /** @type {any} */ (error).problems = problems;
  return error;
}

/** @param {Context} ctx @param {string[]} argv */
function help(ctx, argv) {
  if (argv.filter((arg) => !['--help', '-h', '--json'].includes(arg)).length) throw new UsageError('usage: plan --help');
  if (argv.includes('--json')) {
    ctx.out(JSON.stringify({ commands: COMMANDS }, null, 2));
    return 0;
  }
  const line = (item) => `  ${item.usage.padEnd(16)}  ${item.what}`;
  ctx.out([
    'plan: the plan of Silo, in plan/. The model is plan/README.md.',
    '',
    ...COMMANDS.map(line),
    '',
    'plan, show and check take --json. Exit codes: 0 done, 1 refused or errors found, 2 wrong use.',
  ].join('\n'));
  return 0;
}

/** The plan object of the checkout, or the reader's problems. @param {string} root */
function readPlan(root) {
  const { plan, problems } = parsePlan(planFiles(root));
  if (problems.length) throw new PlanError(problems);
  return plan;
}

/** The entries in the order of order.json. @param {any} plan */
const ordered = (plan) => plan.stages.flatMap((stage) => stage.entries).filter((id, i, all) => Object.hasOwn(plan.entries, id) && all.indexOf(id) === i).map((id) => plan.entries[id]);

/** @param {Context} ctx @param {string[]} argv */
function next(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: plan');
  const plan = readPlan(findRoot(ctx.cwd));
  const entries = ordered(plan);
  const first = entries.find((entry) => entry.verdict === undefined);
  const features = entries.filter((entry) => entry.kind === 'feature');
  const ready = features.filter((entry) => entryStatus(entry, plan.entries) === 'ready').map(({ id, name }) => ({ id, name }));
  const waiting = features.filter((entry) => entryStatus(entry, plan.entries) === 'taken').map(({ id, name }) => ({ id, name, for: waitingFor(plan.entries[id], plan.entries) }));
  const nextOne = first && {
    id: first.id,
    kind: first.kind,
    part: first.part,
    stage: first.stage,
    position: first.position,
    ...(first.kind === 'decision' ? { asks: first.asks, ...(first.answer !== undefined ? { answer: first.answer } : {}) } : { name: first.name, what: first.what }),
    ...(first.suggest ? { suggest: first.suggest } : {}),
  };
  if (flags.has('json')) {
    ctx.out(JSON.stringify({ next: nextOne ?? null, ready, waiting }, null, 2));
    return 0;
  }
  const lines = [];
  if (!nextOne) lines.push('Next: nothing. Every entry has a verdict.');
  else {
    lines.push(`Next: ${nextOne.kind} ${nextOne.id}${nextOne.part ? ` (${nextOne.part})` : ''}, stage ${nextOne.stage}`);
    if (nextOne.kind === 'decision') {
      lines.push(`  asks: ${nextOne.asks}`);
      lines.push(nextOne.answer !== undefined ? `  answer: ${nextOne.answer}` : '  no answer yet');
    } else lines.push(`  ${nextOne.name}: ${nextOne.what}`);
    if (nextOne.suggest) lines.push(`  suggest: ${nextOne.suggest.verdict}. ${nextOne.suggest.why}`);
  }
  lines.push('', ready.length ? 'Ready to build:' : 'Ready to build: none.');
  for (const feature of ready) lines.push(`  ${feature.id}  ${feature.name}`);
  lines.push('', waiting.length ? 'Waiting:' : 'Waiting: none.');
  for (const feature of waiting) lines.push(`  ${feature.id}  waits for ${feature.for.join(', ')}`);
  ctx.out(lines.join('\n'));
  return 0;
}

const PLACE_KEYS = ['id', 'kind', 'part', 'stage', 'position', 'notes'];

/** @param {Context} ctx @param {string[]} argv */
function show(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length !== 1) throw new UsageError('usage: plan show <id>');
  const [id] = positional;
  const plan = readPlan(findRoot(ctx.cwd));
  const report = Object.hasOwn(plan.entries, id) ? showEntry(plan, plan.entries[id]) : showSentence(plan, id);
  if (!report) throw new Refused(`nothing in the plan has the id "${id}"`);
  if (flags.has('json')) ctx.out(JSON.stringify(report.json, null, 2));
  else ctx.out(report.text.join('\n'));
  return 0;
}

/** @param {unknown} value @returns {string} */
const inline = (value) => (typeof value === 'string' ? value : Array.isArray(value) && value.every((v) => typeof v === 'string') ? value.join(', ') : JSON.stringify(value));

/** @param {any} plan @param {any} entry */
function showEntry(plan, entry) {
  const total = ordered(plan).length;
  const status = (other) => entryStatus(other, plan.entries);
  const keys = Object.fromEntries(Object.entries(entry).filter(([key]) => !PLACE_KEYS.includes(key)));
  const needs = (entry.needs ?? []).map((need) => ({ id: need, status: Object.hasOwn(plan.entries, need) ? status(plan.entries[need]) : 'missing' }));
  const neededBy = Object.values(plan.entries).filter((other) => (other.needs ?? []).includes(entry.id)).map((other) => ({ id: other.id, status: status(other) }));
  const keeps = new Set(entry.keeps ?? []);
  const properties = plan.properties.filter((property) => property.part === null || property.part === entry.part || keeps.has(property.id));
  const needed = new Set(entry.needs ?? []);
  const decisions = Object.values(plan.entries)
    .filter((other) => other.id !== entry.id && other.kind === 'decision' && other.verdict === 'take' && (other.part === null || other.part === entry.part || needed.has(other.id)))
    .map(({ id, part, asks, answer }) => ({ id, part, asks, ...(answer !== undefined ? { answer } : {}) }));
  const json = {
    kind: entry.kind,
    id: entry.id,
    part: entry.part,
    stage: entry.stage,
    position: entry.position,
    status: status(entry),
    keys,
    needs,
    needed_by: neededBy,
    ...(entry.notes !== undefined ? { notes: entry.notes } : {}),
    binds: { principles: plan.principles, properties, decisions },
  };
  const text = [`${entry.kind} ${entry.id}${entry.part ? ` (${entry.part})` : ''}: ${json.status}, stage ${entry.stage}, ${entry.position} of ${total}`, ''];
  for (const [key, value] of Object.entries(keys)) {
    if (Array.isArray(value) && value.some((item) => typeof item !== 'string')) {
      text.push(`${key}:`);
      for (const item of value) text.push(`  ${JSON.stringify(item)}`);
    } else text.push(`${key}: ${inline(value)}`);
  }
  text.push('', needs.length ? 'Needs:' : 'Needs: nothing.');
  for (const need of needs) text.push(`  ${need.id}  ${need.status}`);
  text.push('', neededBy.length ? 'Needed by:' : 'Needed by: nothing.');
  for (const other of neededBy) text.push(`  ${other.id}  ${other.status}`);
  text.push('', 'Binds it:');
  for (const principle of plan.principles) text.push(`  principle ${principle.id}: ${principle.says} Asks: ${principle.asks}`);
  for (const property of properties) text.push(`  property ${property.id}${property.part ? ` (${property.part})` : ''}: ${property.says}`);
  for (const decision of decisions) text.push(`  decision ${decision.id}${decision.part ? ` (${decision.part})` : ''}: ${decision.answer ?? decision.asks}`);
  if (entry.notes !== undefined) text.push('', 'Notes:', '', entry.notes.trimEnd());
  return { json, text };
}

/** @param {any} plan @param {string} id */
function showSentence(plan, id) {
  const found = [
    ...plan.words.filter((word) => word.id === id).map((word) => ({ kind: 'word', list: 'words', sentence: word })),
    ...plan.principles.filter((principle) => principle.id === id).map((principle) => ({ kind: 'principle', list: 'principles', sentence: principle })),
    ...plan.properties.filter((property) => property.id === id).map((property) => ({ kind: 'property', list: property.part ? `${property.part}/properties` : 'properties', sentence: property })),
  ][0];
  if (!found) return null;
  const entries = Object.values(plan.entries);
  const addedBy = entries.filter((entry) => (entry.adds ?? []).includes(id)).map((entry) => entry.id);
  const keptBy = entries.filter((entry) => (entry.keeps ?? []).includes(id)).map((entry) => entry.id);
  const setBy = entries.filter((entry) => (entry.sets ?? []).some((item) => item?.id === id || (item?.word !== undefined && String(item.word).toLowerCase().replaceAll(' ', '-') === id))).map((entry) => entry.id);
  const removedBy = entries.filter((entry) => (entry.removes ?? []).includes(id)).map((entry) => entry.id);
  const { id: _id, part: _part, ...keys } = found.sentence;
  const json = { kind: found.kind, id, list: found.list, sentence: keys, added_by: addedBy[0] ?? null, kept_by: keptBy, set_by: setBy, removed_by: removedBy };
  const text = [`${found.kind} ${id}, in ${found.list}`, ''];
  for (const [key, value] of Object.entries(keys)) text.push(`${key}: ${inline(value)}`);
  text.push('', `Added by: ${json.added_by ?? 'nothing yet'}`);
  text.push(`Kept by: ${keptBy.length ? keptBy.join(', ') : 'nothing'}`);
  if (setBy.length) text.push(`Would be replaced by: ${setBy.join(', ')}`);
  if (removedBy.length) text.push(`Would be removed by: ${removedBy.join(', ')}`);
  return { json, text };
}

/** @param {Context} ctx @param {string[]} argv */
function check(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: plan check');
  const root = findRoot(ctx.cwd);
  const files = planFiles(root);
  const { errors, notes } = checkFiles(files, { exists: existsIn(root) });
  const base = baseFiles(root);
  if (base) errors.push(...checkChange(files, base.files, { ruledByOwner: ruledByOwner(root, base.ref) }).errors);
  else notes.push({ file: 'plan/', field: '-', message: 'no git base (origin/main or HEAD): entries changed under their verdict were not looked for', fix: 'run plan check in a git checkout' });
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: errors.length === 0, errors, notes }, null, 2));
  else {
    for (const problem of errors) ctx.out(formatProblem(problem));
    for (const note of notes) ctx.out(`note: ${formatProblem(note)}`);
    if (errors.length) ctx.err(`plan check: ${errors.length} error${errors.length === 1 ? '' : 's'}`);
    else ctx.out('plan check: ok');
  }
  return errors.length ? 1 : 0;
}

/**
 * `plan take <id>` and `plan drop <id>`: the pure function on the files, then the changed files
 * written and committed alone, subject `Owner: <verdict> <id>`.
 * @param {Context} ctx @param {string[]} argv @param {'take' | 'drop'} kind
 */
function verdict(ctx, argv, kind) {
  const { positional, flags } = parseArgs(argv, {});
  if (flags.has('json')) throw new UsageError(`plan ${kind} takes no --json`);
  if (positional.length !== 1) throw new UsageError(`usage: plan ${kind} <id>`);
  const [id] = positional;
  const root = findRoot(ctx.cwd);
  if (git(root, ['rev-parse', '--show-toplevel']) === null) throw new Refused(`${root} is not a git checkout: a verdict is a commit`);
  const dirty = uncommitted(root);
  if (dirty.length) throw new Refused(`plan/ has uncommitted changes (${dirty.join(', ')}): commit or discard them first`);

  const files = planFiles(root);
  const outcome = (kind === 'take' ? take : drop)(files, id, { exists: existsIn(root) });
  if (outcome.status === 'refused') throw refusal(outcome.message, outcome.errors);
  if (outcome.status === 'already') {
    ctx.out(`plan ${kind}: ${outcome.message}`);
    return 0;
  }
  // Rule 11 against the merged plan: the plan must pass it before the verdict, as plan check would.
  // The entry ruled on is left out: the verdict this commit records lifts the rule for it.
  const base = baseFiles(root);
  if (base) {
    const owner = ruledByOwner(root, base.ref);
    const changed = checkChange(files, base.files, { ruledByOwner: (other, file) => other === id || owner(other, file) }).errors;
    if (changed.length) throw refusal(`the plan fails the check against ${base.ref} (${changed.length} error${changed.length === 1 ? '' : 's'}); plan check lists them`, changed);
  }

  const paths = [...outcome.changed.keys()].map((name) => `plan/${name}`);
  /**
   * Puts back every file written so far, as it was: a failed verdict leaves the checkout unchanged.
   * @returns {string} what could not be put back, for the message; empty when all was.
   */
  const restore = (names) => {
    const left = [];
    for (const name of names) {
      const path = join(root, 'plan', name);
      try {
        // The file that failed may be untouched (it could not be opened): leave it as it is then.
        if (!files.has(name)) rmSync(path, { force: true });
        else if (readOr(path) !== files.get(name)) writeFileSync(path, /** @type {string} */ (files.get(name)));
      } catch {
        left.push(`plan/${name}`);
      }
    }
    return left.length ? ` and ${left.join(', ')} could not be put back: restore ${left.length === 1 ? 'it' : 'them'} with git checkout` : '';
  };
  /** @type {string[]} */
  const written = [];
  try {
    for (const [name, text] of outcome.changed) {
      const path = join(root, 'plan', name);
      written.push(name);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, text);
    }
  } catch (error) {
    const left = restore(written);
    throw new Refused(`could not write plan/${written.at(-1)} (${/** @type {Error} */ (error).message}); ${left ? `the files written were put back${left}` : 'nothing changed'}`);
  }
  const subject = `Owner: ${kind} ${id}`;
  const committed = git(root, ['add', '--', ...paths]) !== null && git(root, ['commit', '-q', '-m', subject, '--', ...paths]) !== null;
  if (!committed) {
    git(root, ['reset', '-q', '--', ...paths]);
    const left = restore(outcome.changed.keys());
    throw new Refused(`git could not commit "${subject}"; ${left ? `the files written were put back${left}` : 'nothing changed'}`);
  }
  ctx.out([`${subject}`, ...paths.map((path) => `  ${path}`)].join('\n'));
  return 0;
}
