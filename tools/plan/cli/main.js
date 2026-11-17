// `plan`: the commands. Exit codes: 0 done, 1 refused or errors found, 2 wrong use, 3 live state could not be read.

import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { checkFiles, formatProblem } from '../check/index.js';
import { areaGroups, statusLine } from '../model/areas.js';
import { canonicalFeature, canonicalOrder, canonicalText } from '../model/canonical.js';
import { GONE_STAGE, ID_PATTERN, today, verdictLabel } from '../model/names.js';
import { featureStatus, isReady, waitingFor } from '../model/status.js';
import { createLive } from '../live/index.js';
import { PlanError } from '../read/index.js';
import { decline, decide, start, stop } from '../github/actions.js';
import { GitHub } from '../github/client.js';
import { appCredentials } from '../github/credentials.js';
import { doctor } from '../github/doctor.js';
import { Failure, NoIssue, Unavailable } from '../github/errors.js';
import { macKeychain } from '../github/keychain.js';
import { DEFAULT_API, DEFAULT_ORG } from '../github/names.js';
import { commitVerdicts, readVerdicts, writeVerdicts } from '../github/pull.js';
import { setup, store } from '../github/setup.js';
import { sync } from '../github/sync.js';
import { DEFAULT_REPO, DEFAULT_SITE, DEFAULT_WEB, VERDICTS } from '../model/names.js';
import { Refused, UsageError, parseArgs } from './args.js';
import { findRoot, git, knownAreas, latestVerdicts, loadPlan, writeJson } from './repo.js';

export const COMMANDS = [
  { name: 'status', usage: 'plan status', what: 'Where things stand: latest verdicts, the next to decide, what is built and taken.' },
  { name: 'show', usage: 'plan show <id>', what: 'Everything about one feature.' },
  { name: 'add', usage: 'plan add <id> --name "…" --area <area> (--after <id> | --first-in <stage>) --what "…" [--from <issue>]', what: 'A new feature file and its place in order.json.' },
  { name: 'finish', usage: 'plan finish <id>', what: 'Writes built (today, UTC) in a taken feature whose needs are built.' },
  { name: 'check', usage: 'plan check', what: 'The offline checks of plan/.' },
  { name: 'decide', usage: 'plan decide <id> take|later|drop|clear', what: 'A verdict the owner gave in conversation, recorded on GitHub.' },
  { name: 'start', usage: 'plan start <id> [--branch <branch>]', what: 'Claims a feature for building.' },
  { name: 'stop', usage: 'plan stop <id>', what: 'Releases the claim without finishing.' },
  { name: 'decline', usage: 'plan decline <issue> --why "…"', what: 'Closes a suggestion as not planned.' },
  { name: 'sync', usage: 'plan sync [--dry-run]', what: 'The plan to GitHub.' },
  { name: 'pull', usage: 'plan pull [--write | --commit]', what: "The owner's verdicts from GitHub to the files." },
  { name: 'setup', usage: 'plan setup [--org <org>] [--site <url>] [--replace]', what: 'Creates the Workshop GitHub App.' },
  { name: 'store', usage: 'plan store [--replace]', what: "Stores the App's credentials from vercel.env in the Keychain." },
  { name: 'doctor', usage: 'plan doctor', what: 'Checks the credentials and access.' },
];

/**
 * What touches the world besides the checkout: GitHub, the site, the Keychain, HOME.
 * @typedef {{ env: Record<string, string | undefined>, home: string, keychain: any, fetch: typeof fetch, api: string, web: string, repo: string, site: string,
 *   writeDelayMs: number, sleep?: (ms: number) => Promise<void>, openBrowser: (url: string) => void, setupTimeoutMs?: number }} Deps
 */
/**
 * @typedef {{ cwd: string, now: Date, live: import('../live/index.js').Live, deps: Deps, out: (text: string) => void, err: (text: string) => void }} Context
 */

/** @returns {Deps} */
export function defaultDeps() {
  const env = process.env;
  return {
    env,
    home: homedir(),
    keychain: macKeychain,
    fetch: globalThis.fetch,
    api: env.WORKSHOP_GITHUB_API || DEFAULT_API,
    web: DEFAULT_WEB,
    repo: env.WORKSHOP_REPO || DEFAULT_REPO,
    site: env.PLAN_SITE || DEFAULT_SITE,
    writeDelayMs: 1000,
    openBrowser: (url) => spawn('open', [url], { stdio: 'ignore', detached: true }).unref(),
  };
}

/** @param {string[]} argv @param {Partial<Context> & { deps?: Partial<Deps> }} [context] @returns {Promise<number>} the exit code */
export async function main(argv, context = {}) {
  const deps = { ...defaultDeps(), ...context.deps };
  /** @type {Context} */
  const ctx = {
    cwd: process.cwd(),
    now: new Date(),
    out: (text) => process.stdout.write(`${text}\n`),
    err: (text) => process.stderr.write(`${text}\n`),
    ...context,
    deps,
    live: context.live ?? createLive(deps),
  };
  // --json may come before or after the command name; the command always sees it after.
  const json = argv.includes('--json');
  const [command, ...others] = argv.filter((arg) => arg !== '--json');
  const rest = json ? [...others, '--json'] : others;
  try {
    switch (command) {
      case undefined: return list(ctx, json);
      case 'status': return await status(ctx, rest);
      case 'show': return await show(ctx, rest);
      case 'add': return add(ctx, rest);
      case 'finish': return finish(ctx, rest);
      case 'check': return check(ctx, rest);
      case 'decide': return await decideCommand(ctx, rest);
      case 'start': return await startCommand(ctx, rest);
      case 'stop': return await stopCommand(ctx, rest);
      case 'decline': return await declineCommand(ctx, rest);
      case 'sync': return await syncCommand(ctx, rest);
      case 'pull': return await pullCommand(ctx, rest);
      case 'setup': return await setupCommand(ctx, rest);
      case 'store': return storeCommand(ctx, rest);
      case 'doctor': return await doctorCommand(ctx, rest);
      default: throw new UsageError(`unknown command "${command}"; plan alone lists the commands`);
    }
  } catch (error) {
    const code = error instanceof UsageError ? 2 : error instanceof Unavailable ? 3 : error instanceof Refused || error instanceof PlanError || error instanceof Failure ? 1 : null;
    // Anything else is reported by its first line only: no stack, no body that might carry a value.
    const message = code === null ? String(/** @type {any} */ (error)?.message ?? error).split('\n')[0] : /** @type {Error} */ (error).message;
    if (error instanceof PlanError) {
      if (json) ctx.out(JSON.stringify({ ok: false, error: message, errors: error.problems }, null, 2));
      else for (const problem of error.problems) ctx.err(formatProblem(problem));
    } else if (json) ctx.out(JSON.stringify({ ok: false, error: message }, null, 2));
    else ctx.err(`plan: ${message}`);
    return code ?? 1;
  }
}

/** @param {Context} ctx @param {boolean} json */
function list(ctx, json) {
  if (json) {
    ctx.out(JSON.stringify({ commands: COMMANDS.map(({ name, usage, what }) => ({ name, usage, what })) }, null, 2));
    return 0;
  }
  const line = (item) => `  ${item.usage.padEnd(40)}  ${item.what}`;
  ctx.out(['plan: the plan of Silo, in plan/. Every command takes --json.', '', ...COMMANDS.map(line)].join('\n'));
  return 0;
}

/** The plan, when it passes `plan check`; else refuses with the check's errors. @param {string} root */
function validPlan(root) {
  const loaded = loadPlan(root);
  const { errors } = checkFiles(loaded.files);
  if (errors.length) throw new PlanError(errors);
  return loaded;
}

/**
 * The status of each feature. The verdict is always the file's: an agent acts on its checkout. Live
 * state adds only whether it is being built.
 * @param {any} plan @param {import('../live/index.js').LiveState} live
 */
function statusFunction(plan, live) {
  const entries = live.available ? live.entries : {};
  return (id) => {
    const feature = plan.features[id];
    if (!feature) return 'undecided';
    const issue = Object.hasOwn(entries, id) ? entries[id] : null;
    return featureStatus(feature, issue ? { verdict: null, building: issue.building } : null);
  };
}

/**
 * Where the file's verdict and the issue's verdict labels differ, or null when they agree.
 * @param {any} feature @param {import('../live/index.js').LiveEntry | null | undefined} issue
 * @returns {{ file: string | null, github: string[], line: string } | null}
 */
function notInSync(feature, issue) {
  if (!issue) return null;
  const file = feature.verdict ?? null;
  const github = issue.verdicts ?? (issue.verdict ? [issue.verdict] : []);
  if (github.length === (file ? 1 : 0) && (github[0] ?? null) === file) return null;
  const says = file ? `the file says ${file}` : 'the file has no verdict';
  const shows = github.length ? `#${issue.number} shows ${github.map(verdictLabel).join(', ')}` : `#${issue.number} shows no verdict label`;
  return { file, github, line: `${says}, ${shows}` };
}

/** @param {string} iso @param {Date} now */
export function age(iso, now) {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return '';
  const hours = Math.floor((now.getTime() - then) / 3_600_000);
  if (hours < 1) return 'just now';
  if (hours < 24) return hours === 1 ? '1 hour ago' : `${hours} hours ago`;
  const days = Math.floor(hours / 24);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}

const ordered = (plan) => plan.stages.flatMap((stage) => stage.features).filter((id) => plan.features[id]);
const liveNote = (live) => (live.available ? null : { available: false, reason: live.reason });

/** @param {Context} ctx @param {string[]} argv */
async function status(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: plan status');
  const root = findRoot(ctx.cwd);
  const { plan } = validPlan(root);
  const live = await ctx.live.read();
  const statusOf = statusFunction(plan, live);
  const ids = ordered(plan);
  const name = (id) => plan.features[id].name;

  const latest = latestVerdicts(root);
  const next = ids.find((id) => statusOf(id) === 'undecided') ?? null;
  const building = ids.filter((id) => statusOf(id) === 'building');
  const taken = ids.filter((id) => statusOf(id) === 'planned').map((id) => {
    const feature = plan.features[id];
    return { id, name: feature.name, ready: isReady(feature, 'planned', statusOf), waiting: waitingFor(feature, statusOf) };
  });
  const placed = new Set(Object.values(plan.features).map((feature) => feature.suggested_in).filter((n) => n !== undefined));
  const ideas = live.available ? live.suggestions.filter((item) => item.open && !placed.has(item.number)) : null;
  const where = statusLine(areaGroups(plan, statusOf));
  const differs = live.available
    ? ids.map((id) => ({ id, name: name(id), ...notInSync(plan.features[id], Object.hasOwn(live.entries, id) ? live.entries[id] : null) })).filter((item) => item.line)
    : null;

  const result = {
    line: where,
    latest: latest === null ? null : latest.map((item) => ({ ...item, age: age(item.date, ctx.now) })),
    next: next && { id: next, name: name(next) },
    building: live.available ? building.map((id) => ({ id, name: name(id), branch: live.entries[id]?.branch ?? null })) : null,
    taken,
    ideas,
    notInSync: differs,
    live: liveNote(live),
  };
  if (flags.has('json')) ctx.out(JSON.stringify(result, null, 2));
  else {
    const out = [];
    if (where) out.push(where, '');
    out.push('Latest verdicts:');
    if (result.latest === null) out.push('  not available: not a git checkout');
    else if (!result.latest.length) out.push('  none yet');
    else for (const item of result.latest) out.push(`  ${item.subject.slice('Owner: '.length)}  (${item.age}, ${item.commit.slice(0, 7)})`);
    out.push('', `Next to decide: ${next ? `${next}, ${name(next)}` : 'nothing, every feature has a verdict'}`);
    out.push('', 'Being built:');
    if (!live.available) out.push(`  not available: ${live.reason}`);
    else if (!building.length) out.push('  nothing');
    else for (const item of result.building ?? []) out.push(`  ${item.id}, ${item.name}${item.branch ? ` (on ${item.branch})` : ''}`);
    out.push('', 'Taken, not started:');
    if (!taken.length) out.push('  nothing');
    for (const item of taken) out.push(`  ${item.id}, ${item.name}: ${item.ready ? 'ready' : `waits for ${item.waiting.join(', ')}`}`);
    out.push('', 'Ideas not placed:');
    if (!ideas) out.push(`  not available: ${live.available ? '' : live.reason}`);
    else if (!ideas.length) out.push('  none');
    else for (const item of ideas) out.push(`  #${item.number} ${item.title} (${item.up} votes)`);
    if (differs?.length) {
      out.push('', 'Not in sync:');
      for (const item of differs) out.push(`  ${item.id}: ${item.line}`);
    }
    ctx.out(out.join('\n'));
  }
  return !live.available && live.failed ? 3 : 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function show(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length !== 1) throw new UsageError('usage: plan show <id>');
  const [id] = positional;
  const root = findRoot(ctx.cwd);
  const { plan } = validPlan(root);
  const feature = plan.features[id];
  if (!feature) throw new Refused(`no feature "${id}" in plan/features/`);
  const live = await ctx.live.read({ issue: id });
  const statusOf = statusFunction(plan, live);
  const ids = ordered(plan);
  const stage = plan.stages.find((item) => item.id === feature.stage);
  const area = plan.areas.find((item) => item.id === feature.area);
  const needs = feature.needs.map((need) => ({ id: need, name: plan.features[need]?.name ?? null, status: statusOf(need) }));
  const neededBy = ids.filter((other) => plan.features[other].needs?.includes(id)).map((other) => ({ id: other, name: plan.features[other].name, status: statusOf(other) }));
  const decisions = plan.decisions.filter((decision) => !decision.about || (feature.area !== undefined && decision.about.includes(feature.area)));
  const issue = live.available ? (Object.hasOwn(live.entries, id) ? live.entries[id] : null) : undefined;
  const thread = issue
    ? [
        ...issue.comments.map((comment) => ({ kind: 'comment', createdAt: comment.createdAt, author: comment.author, body: comment.body })),
        ...issue.history.map((line) => ({ kind: 'history', createdAt: line.createdAt, url: line.url ?? null, change: line.change })),
      ].sort((a, b) => a.createdAt.localeCompare(b.createdAt))
    : [];

  const differs = issue ? notInSync(feature, issue) : null;
  const { notes, stage: _stage, position, id: _id, ...fields } = feature;
  const status = statusOf(id);
  const result = {
    id,
    fields,
    stage: stage ? { id: stage.id, title: stage.title } : null,
    position,
    of: ids.length,
    status,
    ready: isReady(feature, status, statusOf),
    needs,
    neededBy,
    decisions,
    notes,
    issue: issue === undefined ? null : issue && { number: issue.number, url: issue.url, up: issue.up, building: issue.building, thread },
    notInSync: differs,
    live: liveNote(live),
  };
  if (flags.has('json')) ctx.out(JSON.stringify(result, null, 2));
  else {
    const out = [`${id}: ${feature.name}`, `  ${feature.what}`, ''];
    out.push(`Status: ${status}${status === 'planned' ? (result.ready ? ', ready to build' : `, waits for ${waitingFor(feature, statusOf).join(', ')}`) : ''}`);
    out.push(`Area: ${area ? `${area.id} (${area.title})` : 'none'}`);
    out.push(`Stage: ${stage ? `${stage.id} (${stage.title})` : 'not placed'}, position ${position ?? '-'} of ${ids.length}`);
    if (feature.suggest) out.push(`Suggested: ${feature.suggest.verdict}${feature.suggest.why ? `. ${feature.suggest.why}` : ''}`);
    if (feature.surfaces) out.push(`Surfaces: ${[feature.surfaces.app && `app ${feature.surfaces.app}`, feature.surfaces.cli && `cli ${feature.surfaces.cli}`].filter(Boolean).join(' · ')}`);
    if (feature.suggested_in !== undefined) out.push(`Suggested in #${feature.suggested_in}`);
    if (feature.built) out.push(`Built: ${feature.built}`);
    if (differs) out.push(`Not in sync: ${differs.line}`);
    out.push('', 'Needs:');
    if (!needs.length) out.push('  nothing');
    for (const need of needs) out.push(`  ${need.id}: ${need.status}`);
    out.push('', 'Needed by:');
    if (!neededBy.length) out.push('  nothing');
    for (const other of neededBy) out.push(`  ${other.id}: ${other.status}`);
    out.push('', 'Decisions:');
    for (const decision of decisions) out.push(`  ${decision.id} (${decision.date}${decision.about ? `, about ${decision.about.join(', ')}` : ''}): ${decision.text}`);
    out.push('', 'Notes:');
    out.push(notes ? notes.trimEnd().split('\n').map((line) => (line ? `  ${line}` : '')).join('\n') : '  none');
    out.push('', 'Issue:');
    if (!live.available) out.push(`  not available: ${live.reason}`);
    else if (!issue) out.push('  none yet');
    else {
      out.push(`  #${issue.number} ${issue.url} (${issue.up} votes)`);
      for (const item of thread) {
        if (item.kind === 'comment') out.push(`  ${item.createdAt.slice(0, 10)} ${item.author}: ${item.body.split('\n')[0]}`);
        else out.push(`  ${item.createdAt.slice(0, 10)} updated in ${item.change.commit.slice(0, 7)}: ${item.change.fields.join(', ')}`);
      }
    }
    ctx.out(out.join('\n'));
  }
  return !live.available && live.failed ? 3 : 0;
}

/** @param {Context} ctx @param {string[]} argv */
function add(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { name: 'value', area: 'value', after: 'value', 'first-in': 'value', what: 'value', from: 'value' });
  const usage = 'usage: plan add <id> --name "…" --area <area> (--after <id> | --first-in <stage>) --what "…" [--from <issue>]';
  if (positional.length !== 1 || values.name === undefined || values.area === undefined || values.what === undefined) throw new UsageError(usage);
  if ((values.after === undefined) === (values['first-in'] === undefined)) throw new UsageError(`give one of --after and --first-in. ${usage}`);
  const [id] = positional;
  if (!ID_PATTERN.test(id)) throw new UsageError(`"${id}" is not a feature id (${ID_PATTERN.source})`);
  if (!values.name.trim()) throw new UsageError('--name is empty');
  if (!values.what.trim()) throw new UsageError('--what is empty');
  let from;
  if (values.from !== undefined) {
    const text = values.from.replace(/^#/, '');
    if (!/^[1-9]\d*$/.test(text)) throw new UsageError(`--from takes an issue number, not "${values.from}"`);
    from = Number(text);
  }

  const root = findRoot(ctx.cwd);
  if (existsSync(join(root, 'plan', 'features', `${id}.json`))) throw new Refused(`the feature "${id}" exists already`);
  const loaded = loadPlan(root);
  const placedIn = Array.isArray(loaded.raw.order?.stages) ? loaded.raw.order.stages.find((item) => Array.isArray(item?.features) && item.features.includes(id)) : undefined;
  if (placedIn) throw new Refused(`"${id}" is already in plan/order.json, in the stage "${placedIn.id}"`);
  const { plan, raw, files } = validPlan(root);
  if (Object.hasOwn(plan.features, id) || existsSync(join(root, 'plan', 'features', `${id}.json`))) throw new Refused(`the feature "${id}" exists already`);
  if (!plan.areas.some((area) => area.id === values.area)) throw new Refused(`no area "${values.area}" in plan/areas.json`);
  const order = structuredClone(raw.order);
  if (!order || !Array.isArray(order.stages)) throw new Refused('plan/order.json has no stages; plan check says why');
  let stageId;
  if (values.after !== undefined) {
    const stage = order.stages.find((item) => Array.isArray(item.features) && item.features.includes(values.after));
    if (!stage) throw new Refused(`no feature "${values.after}" in plan/order.json`);
    stage.features.splice(stage.features.indexOf(values.after) + 1, 0, id);
    stageId = stage.id;
  } else {
    const stage = order.stages.find((item) => item.id === values['first-in']);
    if (!stage || !Array.isArray(stage.features)) throw new Refused(`no stage "${values['first-in']}" in plan/order.json`);
    stage.features.unshift(id);
    stageId = stage.id;
  }
  if (stageId === GONE_STAGE) throw new Refused(`the stage "${GONE_STAGE}" holds old things left behind; place a new feature elsewhere`);

  /** @type {Record<string, unknown>} */
  const feature = { name: values.name.trim(), what: values.what.trim(), area: values.area, needs: [] };
  if (from !== undefined) feature.suggested_in = from;
  const featurePath = `plan/features/${id}.json`;
  // The plan as it would be, checked before anything is written: the command never leaves it failing its check.
  const after = new Map(files);
  after.set(`features/${id}.json`, canonicalText(canonicalFeature(feature)));
  after.set('order.json', canonicalText(canonicalOrder(order)));
  const { errors } = checkFiles(after);
  if (errors.length) throw new PlanError(errors);
  writeJson(root, `features/${id}.json`, canonicalFeature(feature));
  writeJson(root, 'order.json', canonicalOrder(order));
  const paths = [featurePath, 'plan/order.json'];
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: true, id, stage: stageId, paths }, null, 2));
  else ctx.out(paths.join('\n'));
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
function finish(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length !== 1) throw new UsageError('usage: plan finish <id>');
  const [id] = positional;
  const root = findRoot(ctx.cwd);
  const { plan, raw } = validPlan(root);
  if (!Object.hasOwn(raw.features, id)) throw new Refused(`no feature "${id}" in plan/features/`);
  const data = raw.features[id];
  if (data.built) throw new Refused(`${id} is built already (${data.built})`);
  if (data.verdict !== 'take') throw new Refused(`${id} has no verdict take in its file; only a feature the owner took is built`);
  const waiting = (data.needs ?? []).filter((need) => !plan.features[need]?.built);
  if (waiting.length) throw new Refused(`${id} needs ${waiting.join(', ')}, not built yet`);
  const built = today(ctx.now);
  const path = `plan/features/${id}.json`;
  writeJson(root, `features/${id}.json`, canonicalFeature({ ...data, built }));
  // Git reads the path relative to the directory the command was run in.
  const commit = `git commit -m "Built ${id}" -- ${shellWord(relative(resolve(ctx.cwd), join(root, path)))}`;
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: true, id, built, path, commit }, null, 2));
  else ctx.out([`${path}: built ${built}`, commit].join('\n'));
  return 0;
}

/** @param {string} word @returns {string} the word as one shell word */
const shellWord = (word) => (/^[\w./-]+$/.test(word) ? word : `'${word.replaceAll("'", "'\\''")}'`);

/** @param {Context} ctx @param {string[]} argv */
function check(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: plan check');
  const root = findRoot(ctx.cwd);
  const { files, plan } = loadPlan(root);
  const { errors, notes } = checkFiles(files, { knownAreas: knownAreas(root) });
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: errors.length === 0, errors, notes }, null, 2));
  else {
    for (const problem of errors) ctx.out(formatProblem(problem));
    for (const note of notes) ctx.out(note);
    if (!errors.length) ctx.out(`plan check: ok (${Object.keys(plan.features).length} features in ${plan.stages.length} stages, ${plan.areas.length} areas, ${plan.decisions.length} decisions)`);
    else ctx.err(`plan check: ${errors.length} error${errors.length === 1 ? '' : 's'}`);
  }
  return errors.length ? 1 : 0;
}

/** The bot, authenticated on the repository. Exit 3 without credentials or without GitHub. @param {Context} ctx */
async function connect(ctx) {
  const { deps } = ctx;
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: deps.writeDelayMs, sleep: deps.sleep });
  await gh.authenticate(appCredentials(deps));
  return gh;
}

/** @param {Context} ctx @param {string} id the feature's raw file, refusing an unknown id */
function featureFile(ctx, id) {
  const root = findRoot(ctx.cwd);
  const { plan, raw } = validPlan(root);
  if (!Object.hasOwn(raw.features, id)) throw new Refused(`no feature "${id}" in plan/features/`);
  return { root, plan, data: raw.features[id] };
}

/** @param {Context} ctx @param {boolean} json @param {unknown} result @param {string} text */
const emit = (ctx, json, result, text) => ctx.out(json ? JSON.stringify(result, null, 2) : text);

/** @param {Context} ctx @param {string[]} argv */
async function decideCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  const choices = [...VERDICTS, 'clear'];
  if (positional.length !== 2 || !choices.includes(positional[1])) throw new UsageError('usage: plan decide <id> take|later|drop|clear');
  const [id, verdict] = positional;
  const { root, data } = featureFile(ctx, id);
  if (data.built) throw new Refused(`${id} is built (${data.built}); its verdict never changes`);
  const gh = await connect(ctx);
  const fileVerdict = verdictAtMain(root, id);
  if (fileVerdict === undefined) throw new Failure(`the file of ${id} at origin/main could not be read (git fetch origin main failed, or the file is not there); nothing changed`);
  const result = await decide({ gh, id, verdict: /** @type {any} */ (verdict), fileVerdict });
  const shown = verdict === 'clear' ? 'no verdict' : verdict;
  const text = result.changed
    ? 'Recorded on GitHub. The commit follows: git pull.'
    : result.waiting
      ? `GitHub already shows ${verdict}. The commit comes with the next run of the Plan workflow (any push to main starts one).`
      : verdict === 'clear'
        ? `There is no verdict to clear: #${result.issue} and the file at origin/main both show none.`
        : `Already recorded: #${result.issue} and the file at origin/main both show ${shown}.`;
  emit(ctx, flags.has('json'), { ok: true, ...result }, text);
  return 0;
}

/**
 * The verdict of a feature's file at origin/main, after fetching it: null for none, undefined when
 * it cannot be read (the fetch failed, so origin/main may be stale, or the file is not there).
 * @param {string} root @param {string} id
 */
function verdictAtMain(root, id) {
  if (git(root, ['fetch', '--quiet', 'origin', 'main']) === null) return undefined;
  const text = git(root, ['show', `origin/main:plan/features/${id}.json`]);
  if (text === null) return undefined;
  try {
    const data = JSON.parse(text);
    return VERDICTS.includes(data?.verdict) ? data.verdict : null;
  } catch {
    return undefined;
  }
}

/** @param {Context} ctx @param {string[]} argv */
async function startCommand(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { branch: 'value' });
  if (positional.length !== 1) throw new UsageError('usage: plan start <id> [--branch <branch>]');
  const [id] = positional;
  const { root, plan, data } = featureFile(ctx, id);
  if (data.built) throw new Refused(`${id} is built already (${data.built})`);
  if (data.verdict !== 'take') throw new Refused(`${id} has no verdict take in its file; only a feature the owner took is built`);
  const waiting = (data.needs ?? []).filter((need) => !plan.features[need]?.built);
  if (waiting.length) throw new Refused(`${id} needs ${waiting.join(', ')}, not built yet`);
  const branch = values.branch ?? git(root, ['rev-parse', '--abbrev-ref', 'HEAD'])?.trim();
  if (!branch || branch === 'HEAD') throw new Refused('not on a branch: check one out, or pass --branch');
  if (!/^[A-Za-z0-9._/-]{1,200}$/.test(branch)) throw new Refused(`the branch "${branch}" cannot be named in the claim`);
  let gh;
  try {
    gh = await connect(ctx);
  } catch (error) {
    if (!(error instanceof Unavailable)) throw error;
    emit(ctx, flags.has('json'), { ok: true, id, branch, claimed: false, shared: false, reason: error.message }, `The claim is not shared: ${error.message}. Other sessions cannot see that ${id} is being built.`);
    return 0;
  }
  try {
    const result = await start({ gh, id, branch });
    emit(ctx, flags.has('json'), { ok: true, ...result, shared: true }, `Claimed ${id} on #${result.issue}: being built on ${branch}.`);
    return 0;
  } catch (error) {
    if (!(error instanceof NoIssue)) throw error;
    emit(ctx, flags.has('json'), { ok: true, id, branch, claimed: false, shared: false, reason: error.message }, `The claim is not shared: ${error.message}.`);
    return 0;
  }
}

/** @param {Context} ctx @param {string[]} argv */
async function stopCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length !== 1) throw new UsageError('usage: plan stop <id>');
  const [id] = positional;
  featureFile(ctx, id);
  const gh = await connect(ctx);
  const result = await stop({ gh, id });
  emit(ctx, flags.has('json'), { ok: true, ...result }, result.changed ? `${id} is no longer being built (#${result.issue}).` : `${id} is not being built (#${result.issue}); nothing to stop.`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function declineCommand(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { why: 'value' });
  if (positional.length !== 1 || values.why === undefined) throw new UsageError('usage: plan decline <issue> --why "…"');
  const text = positional[0].replace(/^#/, '');
  if (!/^[1-9]\d*$/.test(text)) throw new UsageError(`"${positional[0]}" is not an issue number`);
  const why = values.why.trim();
  if (!why) throw new UsageError('--why is empty');
  const gh = await connect(ctx);
  const result = await decline({ gh, number: Number(text), why });
  emit(ctx, flags.has('json'), { ok: true, ...result }, `Closed #${result.number} as not planned.`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function syncCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, { 'dry-run': 'flag' });
  if (positional.length) throw new UsageError('usage: plan sync [--dry-run]');
  const root = findRoot(ctx.cwd);
  const json = flags.has('json');
  const gh = await connect(ctx);
  const dryRun = flags.has('dry-run');
  const result = await sync({ gh, root, dryRun, log: json ? () => {} : ctx.out, site: ctx.deps.site, web: ctx.deps.web, repo: ctx.deps.repo });
  if (json) ctx.out(JSON.stringify({ ok: !result.stopped, ...result }, null, 2));
  else {
    for (const note of result.notes) ctx.out(`note: ${note}`);
    const w = (word) => (dryRun ? `would ${word}` : word);
    ctx.out(`At ${result.synced.slice(0, 7)}: ${w('create')} ${result.created}, ${w('update')} ${result.updated}, ${w('close')} ${result.closed}, ${w('reopen')} ${result.reopened}, history ${result.history}, suggestions closed ${result.suggestions}, duplicates closed ${result.duplicatesClosed}, no longer in the plan ${result.removed}, labels ${result.labels}, unchanged ${result.unchanged}.`);
  }
  if (result.stopped) {
    ctx.err(`plan: stopped: ${result.stoppedMessage}; the next run resumes`);
    return 3;
  }
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function pullCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, { write: 'flag', commit: 'flag' });
  if (positional.length) throw new UsageError('usage: plan pull [--write | --commit]');
  const root = findRoot(ctx.cwd);
  const { plan, raw } = validPlan(root);
  const gh = await connect(ctx);
  const { changes, reports, duplicates } = await readVerdicts({ gh, root, plan, raw });
  const write = flags.has('write') || flags.has('commit');
  const { written, skipped } = write ? writeVerdicts(root, changes) : { written: [], skipped: [] };
  let commit = null;
  if (flags.has('commit') && written.length) {
    try {
      commit = commitVerdicts(root, written);
    } catch {
      throw new Refused(`the verdicts are written but git commit failed; commit ${written.map((c) => `plan/features/${c.id}.json`).join(' ')} by hand`);
    }
  }
  const result = { ok: true, changes, reports, duplicates, written: write ? written.map((c) => c.id) : null, skipped, commit };
  if (flags.has('json')) ctx.out(JSON.stringify(result, null, 2));
  else {
    const out = [`Verdicts to bring to the files (${changes.length}):`];
    for (const c of changes) out.push(`  ${c.id} (#${c.issue}): ${c.from ?? 'none'} -> ${c.to ?? 'none'}${c.by ? `, by ${c.by}` : ''}`);
    out.push(`Left alone (${reports.length}):`);
    for (const r of reports) out.push(`  ${r.id} (#${r.issue}): ${r.why}`);
    for (const d of duplicates) out.push(`Duplicate: #${d.duplicate} also carries the marker of ${d.id}; #${d.kept} is used`);
    for (const skip of skipped) out.push(`Skipped: ${skip}`);
    if (write) out.push(`Wrote ${written.length} feature file${written.length === 1 ? '' : 's'}.`);
    if (commit) out.push(`Committed ${commit.slice(0, 7)}.`);
    ctx.out(out.join('\n'));
  }
  return 0;
}

/** @param {Context} ctx */
function setupDeps(ctx) {
  return { ...ctx.deps, log: ctx.err };
}

/** @param {Context} ctx @param {string[]} argv */
async function setupCommand(ctx, argv) {
  const { flags, positional, values } = parseArgs(argv, { org: 'value', site: 'value', replace: 'flag' });
  if (positional.length) throw new UsageError('usage: plan setup [--org <org>] [--site <url>] [--replace]');
  const result = await setup({ org: values.org ?? DEFAULT_ORG, site: values.site ?? DEFAULT_SITE, replace: flags.has('replace'), deps: setupDeps(ctx) });
  emit(ctx, flags.has('json'), { ok: true, ...result }, `Created the App ${result.app}. Credentials are in the Keychain; Vercel's variables are in ${result.envFile}.\nInstall it on ${ctx.deps.repo}: ${result.installUrl}`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
function storeCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, { replace: 'flag' });
  if (positional.length) throw new UsageError('usage: plan store [--replace]');
  const result = store({ replace: flags.has('replace'), deps: setupDeps(ctx) });
  emit(ctx, flags.has('json'), { ok: true, ...result }, result.already ? `The Keychain already holds the App credentials of ${result.from}.` : `Stored ${result.stored.join(', ')} from ${result.from} in the Keychain.`);
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
async function doctorCommand(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length) throw new UsageError('usage: plan doctor');
  const { deps } = ctx;
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: 0 });
  const report = await doctor({ deps, gh, credentials: () => appCredentials(deps) });
  const lines = [
    ...Object.entries(report.credentials).map(([name, source]) => `${name}: ${source}`),
    `App: ${report.app ?? 'unknown'}`,
    `Public: ${report.public === true ? 'yes' : report.public === false ? 'no (sign-in will not work for anyone outside its organization)' : 'unknown'}`,
    `Installed on ${deps.repo}: ${report.installed ? 'yes' : 'no'}`,
    `Permissions: ${report.permissions ? Object.entries(report.permissions).map(([k, v]) => `${k} ${v}`).join(', ') || 'none' : 'unknown'}`,
    ...(report.labels ? Object.entries(report.labels).map(([name, ok]) => `Label ${name}: ${ok ? 'yes' : 'missing'}`) : ['Labels: unknown']),
    ...report.problems.map((problem) => `Problem: ${problem}`),
  ];
  emit(ctx, flags.has('json'), { ok: report.problems.length === 0, ...report }, lines.join('\n'));
  return report.problems.length ? 1 : 0;
}
