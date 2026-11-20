// `plan`: the commands. Exit codes: 0 done, 1 refused or errors found, 2 wrong use.
// Nothing here calls the network: the command reads `plan/`, checks it, and commits a verdict.

import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { checkChange, checkFiles, formatProblem } from '../check/index.js';
import { entryStatus, hasSets, replacedBy, waitingFor } from '../model/status.js';
import { PlanError, parsePlan } from '../read/index.js';
import { drop, take } from '../write/index.js';
import { Refused, UsageError, parseArgs } from './args.js';
import { baseFiles, existsIn, findRoot, git, planFiles, ruledByOwner, uncommitted, unwatched } from './repo.js';

export const COMMANDS = [
  { name: '', usage: 'plan', what: 'What is next: the first entry with no verdict, the features ready to build, the features that wait and for what.' },
  { name: 'show', usage: 'plan show <id>', what: 'One entry, part or statement. An entry with what it links to and what binds it (--all: the whole product\'s decisions in full); a part with what is next in it.' },
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

/** One `sets` item on one line: its list, its id or word, and what it says or means. @param {any} item */
const setLine = (item) => {
  const name = item?.id ?? item?.word;
  const text = item?.says ?? item?.means;
  return `${item?.in}: ${name}${text !== undefined ? `: ${text}` : ''}`;
};

/**
 * A decision an entry names in `replaces`, as the owner weighs it: its answer, or the ids it adds
 * when it has none. A name that is no decision of the plan has neither.
 * @param {any} plan @param {string} id
 */
function replacedSummary(plan, id) {
  const decision = Object.hasOwn(plan.entries, id) && plan.entries[id].kind === 'decision' ? plan.entries[id] : undefined;
  if (!decision) return { id };
  return { id, ...(decision.answer !== undefined ? { answer: decision.answer } : { adds: decision.adds ?? [] }) };
}

/** One line of `replaces`: the decision's id and its answer, or what it adds. @param {{ id: string, answer?: string, adds?: string[] }} summary */
const replacedText = (summary) => `${summary.id}: ${summary.answer ?? (summary.adds ? `adds ${summary.adds.join(', ') || 'nothing'}` : 'not a decision of the plan')}`;

/** The decisions an entry names in `replaces`, summed up, or undefined when it names none. @param {any} plan @param {any} entry */
const replacesOf = (plan, entry) => (entry.kind === 'decision' && Array.isArray(entry.replaces) ? entry.replaces.map((id) => replacedSummary(plan, id)) : undefined);

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
    ...(first.kind === 'decision' ? { asks: first.asks, ...(first.answer !== undefined ? { answer: first.answer } : hasSets(first) ? { sets: first.sets } : {}) } : { name: first.name, what: first.what }),
    ...(replacesOf(plan, first) ? { replaces: replacesOf(plan, first) } : {}),
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
      if (nextOne.answer !== undefined) lines.push(`  answer: ${nextOne.answer}`);
      else if (nextOne.sets) {
        // A decision whose proposal is only statements: they stand for its answer.
        lines.push('  sets:');
        for (const item of nextOne.sets) lines.push(`    ${setLine(item)}`);
      } else lines.push('  no answer yet');
      // What a yes puts out of force.
      if (nextOne.replaces) {
        lines.push('  replaces:');
        for (const summary of nextOne.replaces) lines.push(`    ${replacedText(summary)}`);
      }
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
  const { flags, positional } = parseArgs(argv, { all: 'flag' });
  if (positional.length !== 1) throw new UsageError('usage: plan show <id> [--all]');
  const [id] = positional;
  const plan = readPlan(findRoot(ctx.cwd));
  const isEntry = Object.hasOwn(plan.entries, id);
  if (flags.has('all') && !isEntry) throw new UsageError('--all is for an entry: it prints the taken decisions of the whole product in full');
  const part = isEntry ? undefined : plan.parts.find((item) => item?.id === id);
  const report = isEntry ? showEntry(plan, plan.entries[id], flags.has('all')) : part ? showPart(plan, part) : showStatement(plan, id);
  if (!report) throw new Refused(`nothing in the plan has the id "${id}"`);
  if (flags.has('json')) ctx.out(JSON.stringify(report.json, null, 2));
  else ctx.out(report.text.join('\n'));
  return 0;
}

/** @param {unknown} value @returns {string} */
const inline = (value) => (typeof value === 'string' ? value : Array.isArray(value) && value.every((v) => typeof v === 'string') ? value.join(', ') : JSON.stringify(value));

/** Ids joined with commas, on lines of at most `width` characters, each line indented. @param {string[]} ids @param {string} indent */
function wrapIds(ids, indent, width = 100) {
  const lines = [];
  let line = '';
  ids.forEach((id, index) => {
    const word = index < ids.length - 1 ? `${id},` : id;
    if (line && indent.length + line.length + 1 + word.length > width) {
      lines.push(indent + line);
      line = word;
    } else line = line ? `${line} ${word}` : word;
  });
  if (line) lines.push(indent + line);
  return lines;
}

/** A taken decision on one line: its answer, or the ids it adds when it has none. @param {any} decision */
const decisionLine = (decision) => `  decision ${decision.id}${decision.part ? ` (${decision.part})` : ''}: ${decision.answer ?? `adds ${(decision.adds ?? []).join(', ')}`}`;

/**
 * An entry, whole, then what binds it. The taken decisions of the whole product that the entry
 * does not need are shown as ids only, unless `all`: they are many and rarely about the entry.
 * @param {any} plan @param {any} entry @param {boolean} all
 */
function showEntry(plan, entry, all) {
  const total = ordered(plan).length;
  const status = (other) => entryStatus(other, plan.entries);
  const keys = Object.fromEntries(Object.entries(entry).filter(([key]) => !PLACE_KEYS.includes(key)));
  const needs = (entry.needs ?? []).map((need) => {
    if (!Object.hasOwn(plan.entries, need)) return { id: need, status: 'missing' };
    const by = replacedBy(plan.entries[need]);
    return { id: need, status: status(plan.entries[need]), ...(by ? { replaced_by: by } : {}) };
  });
  const replacedByOther = replacedBy(entry);
  const replaces = replacesOf(plan, entry);
  const neededBy = Object.values(plan.entries).filter((other) => (other.needs ?? []).includes(entry.id)).map((other) => ({ id: other.id, status: status(other) }));
  const keeps = new Set(entry.keeps ?? []);
  const properties = plan.properties.filter((property) => property.part === null || property.part === entry.part || keeps.has(property.id));
  const needed = new Set(entry.needs ?? []);
  const decisions = Object.values(plan.entries)
    // In force: taken and not replaced.
    .filter((other) => other.id !== entry.id && other.kind === 'decision' && other.verdict === 'take' && !replacedBy(other) && (other.part === null || other.part === entry.part || needed.has(other.id)))
    .map(({ id, part, asks, answer, adds }) => ({ id, part, asks, ...(answer !== undefined ? { answer } : {}), ...(adds !== undefined ? { adds } : {}) }));
  const json = {
    kind: entry.kind,
    id: entry.id,
    part: entry.part,
    stage: entry.stage,
    position: entry.position,
    status: status(entry),
    ...(replacedByOther ? { replaced_by: replacedByOther } : {}),
    keys,
    ...(replaces ? { replaces } : {}),
    needs,
    needed_by: neededBy,
    ...(entry.notes !== undefined ? { notes: entry.notes } : {}),
    binds: { principles: plan.principles, properties, decisions },
  };
  const text = [`${entry.kind} ${entry.id}${entry.part ? ` (${entry.part})` : ''}: ${json.status}, stage ${entry.stage}, ${entry.position} of ${total}`];
  if (replacedByOther) text.push(`Replaced by: ${replacedByOther}`);
  text.push('');
  for (const [key, value] of Object.entries(keys)) {
    if (key === 'replaces' && replaces) {
      text.push('replaces:');
      for (const summary of replaces) text.push(`  ${replacedText(summary)}`);
    } else if (Array.isArray(value) && value.some((item) => typeof item !== 'string')) {
      text.push(`${key}:`);
      for (const item of value) text.push(`  ${JSON.stringify(item)}`);
    } else text.push(`${key}: ${inline(value)}`);
  }
  text.push('', needs.length ? 'Needs:' : 'Needs: nothing.');
  for (const need of needs) text.push(`  ${need.id}  ${need.replaced_by ? `replaced by ${need.replaced_by}` : need.status}`);
  text.push(neededBy.length ? 'Needed by:' : 'Needed by: nothing.');
  for (const other of neededBy) text.push(`  ${other.id}  ${other.status}`);
  if (entry.notes !== undefined) text.push('', 'Notes:', '', entry.notes.trimEnd());
  // The decisions of the entry's own part and those it needs are about it; the root's are about everything.
  const close = decisions.filter((decision) => (entry.part !== null && decision.part === entry.part) || needed.has(decision.id));
  const whole = decisions.filter((decision) => !close.includes(decision));
  text.push('', 'What binds it:');
  for (const principle of plan.principles) text.push(`  principle ${principle.id}: ${principle.says}`);
  for (const property of properties) text.push(`  property ${property.id}${property.part ? ` (${property.part})` : ''}: ${property.says}`);
  for (const decision of close) text.push(decisionLine(decision));
  if (whole.length && all) {
    text.push('  Taken decisions of the whole product:');
    for (const decision of whole) text.push(`  ${decisionLine(decision)}`);
  } else if (whole.length) {
    text.push(`  Taken decisions of the whole product (plan show ${entry.id} --all prints them):`);
    text.push(...wrapIds(whole.map((decision) => decision.id), '    '));
  }
  return { json, text };
}

/**
 * A part: its title and line, its entries in the order (the taken ones in force, the replaced and
 * the dropped ones as ids, then
 * each one with no verdict, the first marked next), and its properties.
 * @param {any} plan @param {any} part
 */
function showPart(plan, part) {
  const entries = ordered(plan).filter((entry) => entry.part === part.id);
  const replaced = entries.filter((entry) => replacedBy(entry)).map((entry) => entry.id);
  const taken = entries.filter((entry) => entry.verdict === 'take' && !replaced.includes(entry.id)).map((entry) => entry.id);
  const dropped = entries.filter((entry) => entry.verdict === 'drop').map((entry) => entry.id);
  const open = entries.filter((entry) => entry.verdict === undefined).map((entry) => ({
    id: entry.id,
    kind: entry.kind,
    status: entryStatus(entry, plan.entries),
    text: entry.kind === 'decision' ? entry.asks : entry.name,
  }));
  const properties = plan.properties.filter((property) => property.part === part.id).map(({ id, says }) => ({ id, says }));
  // A part may share its id with the word for its concept: plan show on that id shows both.
  const word = plan.words.find((item) => item.id === part.id);
  const json = { kind: 'part', id: part.id, title: part.title, line: part.line, ...(word ? { word: { word: word.word, means: word.means } } : {}), taken, replaced, dropped, next: open[0]?.id ?? null, open, properties };
  const text = [`part ${part.id}: ${part.title}`, part.line, ...(word ? [`The word "${word.word}": ${word.means}`] : []), ''];
  text.push(`Taken: ${taken.length}${taken.length ? '' : '.'}`, ...wrapIds(taken, '  '));
  if (replaced.length) text.push(`Replaced: ${replaced.length}`, ...wrapIds(replaced, '  '));
  if (dropped.length) text.push(`Dropped: ${dropped.length}`, ...wrapIds(dropped, '  '));
  text.push('', open.length ? 'To decide, in order:' : 'To decide: nothing. Every entry has a verdict.');
  const width = Math.max(0, ...open.map((entry) => entry.id.length));
  open.forEach((entry, index) => {
    text.push(`${index === 0 ? 'next' : '    '}  ${entry.id.padEnd(width)}  ${entry.kind.padEnd(8)}  ${entry.status.padEnd(9)}  ${entry.text}`);
  });
  text.push('', properties.length ? 'Properties:' : 'Properties: none.');
  for (const property of properties) text.push(`  ${property.id}: ${property.says}`);
  return { json, text };
}

/** @param {any} plan @param {string} id */
function showStatement(plan, id) {
  const found = [
    ...plan.words.filter((word) => word.id === id).map((word) => ({ kind: 'word', list: 'words', statement: word })),
    ...plan.principles.filter((principle) => principle.id === id).map((principle) => ({ kind: 'principle', list: 'principles', statement: principle })),
    ...plan.properties.filter((property) => property.id === id).map((property) => ({ kind: 'property', list: property.part ? `${property.part}/properties` : 'properties', statement: property })),
  ][0];
  if (!found) return null;
  const entries = Object.values(plan.entries);
  const addedBy = entries.filter((entry) => (entry.adds ?? []).includes(id)).map((entry) => entry.id);
  const keptBy = entries.filter((entry) => (entry.keeps ?? []).includes(id)).map((entry) => entry.id);
  const setBy = entries.filter((entry) => (entry.sets ?? []).some((item) => item?.id === id || (item?.word !== undefined && String(item.word).toLowerCase().replaceAll(' ', '-') === id))).map((entry) => entry.id);
  const removedBy = entries.filter((entry) => (entry.removes ?? []).includes(id)).map((entry) => entry.id);
  const { id: _id, part: _part, ...keys } = found.statement;
  const json = { kind: found.kind, id, list: found.list, statement: keys, added_by: addedBy[0] ?? null, kept_by: keptBy, set_by: setBy, removed_by: removedBy };
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
  const hidden = unwatched(root);
  if (hidden.length) throw new Refused(`git is told not to look at ${hidden.join(', ')}, so it cannot say whether plan/ has uncommitted changes: clear the flag with git update-index --no-assume-unchanged or --no-skip-worktree first`);

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
        // A file the verdict created is removed.
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
  ctx.out([`${subject}`, ...[...outcome.changed.keys()].map((name) => `  plan/${name}`)].join('\n'));
  return 0;
}
