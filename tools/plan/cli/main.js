// `plan`: the commands. Exit codes: 0 done, 1 refused or errors found, 2 wrong use, 3 live state could not be read.

import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../check/index.js';
import { areaGroups, statusLine } from '../model/areas.js';
import { canonicalFeature, canonicalOrder } from '../model/canonical.js';
import { GONE_STAGE, ID_PATTERN, today } from '../model/names.js';
import { featureStatus, isReady, waitingFor } from '../model/status.js';
import { offlineLive } from '../live/index.js';
import { PlanError } from '../read/index.js';
import { Refused, UsageError, parseArgs } from './args.js';
import { findRoot, knownAreas, latestVerdicts, loadPlan, writeJson } from './repo.js';

export const COMMANDS = [
  { name: 'status', usage: 'plan status', what: 'Where things stand: latest verdicts, the next to decide, what is built and taken.' },
  { name: 'show', usage: 'plan show <id>', what: 'Everything about one feature.' },
  { name: 'add', usage: 'plan add <id> --name "…" --area <area> (--after <id> | --first-in <stage>) [--what "…"] [--from <issue>]', what: 'A new feature file and its place in order.json.' },
  { name: 'finish', usage: 'plan finish <id>', what: 'Writes built (today, UTC) in a taken feature whose needs are built.' },
  { name: 'check', usage: 'plan check', what: 'The offline checks of plan/.' },
  { name: 'decide', usage: 'plan decide <id> take|later|drop|clear', what: 'A verdict the owner gave in conversation, recorded on GitHub.', coming: true },
  { name: 'start', usage: 'plan start <id>', what: 'Claims a feature for building.', coming: true },
  { name: 'stop', usage: 'plan stop <id>', what: 'Releases the claim without finishing.', coming: true },
  { name: 'decline', usage: 'plan decline <issue> --why "…"', what: 'Closes a suggestion as not planned.', coming: true },
  { name: 'sync', usage: 'plan sync [--dry-run]', what: 'The plan to GitHub.', coming: true },
  { name: 'pull', usage: 'plan pull [--write | --commit]', what: "The owner's verdicts from GitHub to the files.", coming: true },
  { name: 'setup', usage: 'plan setup', what: 'Creates the Workshop GitHub App.', coming: true },
  { name: 'store', usage: 'plan store', what: "Stores the App's credentials.", coming: true },
  { name: 'doctor', usage: 'plan doctor', what: 'Checks the credentials and access.', coming: true },
];

/**
 * @typedef {{ cwd: string, now: Date, live: import('../live/index.js').Live, out: (text: string) => void, err: (text: string) => void }} Context
 */

/** @param {string[]} argv @param {Partial<Context>} [context] @returns {Promise<number>} the exit code */
export async function main(argv, context = {}) {
  /** @type {Context} */
  const ctx = {
    cwd: process.cwd(),
    now: new Date(),
    live: offlineLive(),
    out: (text) => process.stdout.write(`${text}\n`),
    err: (text) => process.stderr.write(`${text}\n`),
    ...context,
  };
  const json = argv.includes('--json');
  const [command, ...rest] = argv.filter((arg, index) => index > 0 || arg !== '--json');
  try {
    switch (command) {
      case undefined: return list(ctx, json);
      case 'status': return await status(ctx, rest);
      case 'show': return await show(ctx, rest);
      case 'add': return add(ctx, rest);
      case 'finish': return finish(ctx, rest);
      case 'check': return check(ctx, rest);
      default: {
        const coming = COMMANDS.find((item) => item.name === command && item.coming);
        throw new UsageError(coming ? `plan ${command} is not built yet` : `unknown command "${command}"; plan alone lists the commands`);
      }
    }
  } catch (error) {
    const code = error instanceof UsageError ? 2 : error instanceof Refused || error instanceof PlanError ? 1 : null;
    if (code === null) throw error;
    if (json) ctx.out(JSON.stringify({ ok: false, error: error.message }, null, 2));
    else ctx.err(`plan: ${error.message}`);
    return code;
  }
}

/** @param {Context} ctx @param {boolean} json */
function list(ctx, json) {
  if (json) {
    ctx.out(JSON.stringify({ commands: COMMANDS.map(({ name, usage, what, coming }) => ({ name, usage, what, coming: Boolean(coming) })) }, null, 2));
    return 0;
  }
  const line = (item) => `  ${item.usage.padEnd(40)}  ${item.what}`;
  ctx.out(['plan: the plan of Silo, in plan/. Every command takes --json.', '', ...COMMANDS.filter((item) => !item.coming).map(line), '', 'Coming with the GitHub side:', ...COMMANDS.filter((item) => item.coming).map(line)].join('\n'));
  return 0;
}

/** The plan, strictly: refuses when a file is missing or is not JSON. @param {string} root */
function strictPlan(root) {
  const loaded = loadPlan(root);
  if (loaded.problems.length) throw new PlanError(loaded.problems);
  return loaded;
}

/** @param {any} plan @param {import('../live/index.js').LiveState} live */
function statusFunction(plan, live) {
  const entries = live.available ? live.entries : {};
  return (id) => {
    const feature = plan.features[id];
    if (!feature) return 'undecided';
    const issue = Object.hasOwn(entries, id) ? entries[id] : null;
    return featureStatus(feature, issue ? { verdict: issue.verdict, building: issue.building } : null);
  };
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
  const { plan } = strictPlan(root);
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

  const result = {
    line: where,
    latest: latest === null ? null : latest.map((item) => ({ ...item, age: age(item.date, ctx.now) })),
    next: next && { id: next, name: name(next) },
    building: live.available ? building.map((id) => ({ id, name: name(id), branch: live.entries[id]?.branch ?? null })) : null,
    taken,
    ideas,
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
  const { plan } = strictPlan(root);
  const feature = plan.features[id];
  if (!feature) throw new Refused(`no feature "${id}" in plan/features/`);
  const live = await ctx.live.read();
  const statusOf = statusFunction(plan, live);
  const ids = ordered(plan);
  const stage = plan.stages.find((item) => item.id === feature.stage);
  const area = plan.areas.find((item) => item.id === feature.area);
  const needs = feature.needs.map((need) => ({ id: need, name: plan.features[need]?.name ?? null, status: statusOf(need) }));
  const neededBy = ids.filter((other) => plan.features[other].needs?.includes(id)).map((other) => ({ id: other, name: plan.features[other].name, status: statusOf(other) }));
  const decisions = plan.decisions.filter((decision) => !decision.about || decision.about.includes(id) || (feature.area && decision.about.includes(feature.area)));
  const issue = live.available ? (Object.hasOwn(live.entries, id) ? live.entries[id] : null) : undefined;
  const thread = issue
    ? [
        ...issue.comments.map((comment) => ({ kind: 'comment', createdAt: comment.createdAt, author: comment.author, body: comment.body })),
        ...issue.history.map((line) => ({ kind: 'history', createdAt: line.createdAt, url: line.url ?? null, change: line.change })),
      ].sort((a, b) => a.createdAt.localeCompare(b.createdAt))
    : [];

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
    issue: issue === undefined ? null : issue && { number: issue.number, url: issue.url, up: issue.up, verdict: issue.verdict, building: issue.building, thread },
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
  const usage = 'usage: plan add <id> --name "…" --area <area> (--after <id> | --first-in <stage>) [--what "…"] [--from <issue>]';
  if (positional.length !== 1 || values.name === undefined || values.area === undefined) throw new UsageError(usage);
  if ((values.after === undefined) === (values['first-in'] === undefined)) throw new UsageError(`give one of --after and --first-in. ${usage}`);
  const [id] = positional;
  if (!ID_PATTERN.test(id)) throw new UsageError(`"${id}" is not a feature id (${ID_PATTERN.source})`);
  if (!values.name.trim()) throw new UsageError('--name is empty');
  let from;
  if (values.from !== undefined) {
    const text = values.from.replace(/^#/, '');
    if (!/^[1-9]\d*$/.test(text)) throw new UsageError(`--from takes an issue number, not "${values.from}"`);
    from = Number(text);
  }

  const root = findRoot(ctx.cwd);
  const { plan, raw } = strictPlan(root);
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
  const feature = { name: values.name.trim(), what: (values.what ?? '').trim(), area: values.area, needs: [] };
  if (from !== undefined) feature.suggested_in = from;
  const featurePath = `plan/features/${id}.json`;
  writeJson(root, `features/${id}.json`, canonicalFeature(feature));
  writeJson(root, 'order.json', canonicalOrder(order));
  const paths = [featurePath, 'plan/order.json'];
  const todo = feature.what ? null : `write what it does in ${featurePath}: plan check fails until then`;
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: true, id, stage: stageId, paths, todo }, null, 2));
  else ctx.out([...paths, ...(todo ? [`Then ${todo}.`] : [])].join('\n'));
  return 0;
}

/** @param {Context} ctx @param {string[]} argv */
function finish(ctx, argv) {
  const { flags, positional } = parseArgs(argv, {});
  if (positional.length !== 1) throw new UsageError('usage: plan finish <id>');
  const [id] = positional;
  const root = findRoot(ctx.cwd);
  const { plan, raw } = strictPlan(root);
  if (!Object.hasOwn(raw.features, id)) throw new Refused(`no feature "${id}" in plan/features/`);
  const data = raw.features[id];
  if (data.built) throw new Refused(`${id} is built already (${data.built})`);
  if (data.verdict !== 'take') throw new Refused(`${id} has no verdict take in its file; only a feature the owner took is built`);
  const waiting = (data.needs ?? []).filter((need) => !plan.features[need]?.built);
  if (waiting.length) throw new Refused(`${id} needs ${waiting.join(', ')}, not built yet`);
  const built = today(ctx.now);
  const path = `plan/features/${id}.json`;
  writeJson(root, `features/${id}.json`, canonicalFeature({ ...data, built }));
  const commit = `git commit -m "Built ${id}" -- ${path}`;
  if (flags.has('json')) ctx.out(JSON.stringify({ ok: true, id, built, path, commit }, null, 2));
  else ctx.out([`${path}: built ${built}`, commit].join('\n'));
  return 0;
}

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
