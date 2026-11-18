// `plan check`: the offline checks of `plan/`. Each problem is { file, field, message, fix }.

import {
  AREA_KEYS, DECISION_KEYS, FEATURE_KEYS, GONE_STAGE, ID_PATTERN, ORDER_KEYS, PLAN_FILES, STAGE_KEYS, SUGGEST_KEYS, SURFACES_KEYS, VERDICTS, isDate,
} from '../model/names.js';
import { canonicalAreas, canonicalDecisions, canonicalFeature, canonicalOrder, canonicalText } from '../model/canonical.js';
import { isHidden, parsePlan } from '../read/index.js';

/** @typedef {{ file: string, field: string, message: string, fix: string }} Problem */

/** @param {Problem} problem */
export const formatProblem = (p) => `${p.file}: ${p.field}: ${p.message}. Fix: ${p.fix}`;

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const isText = (value) => typeof value === 'string' && value.trim() !== '';
const FEATURE_FILE = /^features\/([^/]+)\.(json|md)$/;
const TOP_FILES = new Set([PLAN_FILES.product, PLAN_FILES.design, PLAN_FILES.areas, PLAN_FILES.order, PLAN_FILES.decisions]);

/**
 * @param {Map<string, string>} files the files of `plan/`, by path relative to it
 * @param {{ knownAreas?: string[] | null }} [options] the area ids of the merged plan, to mention the map for a new one
 * @returns {{ errors: Problem[], notes: string[] }}
 */
export function checkFiles(files, { knownAreas = null } = {}) {
  const { problems, raw } = parsePlan(files);
  /** @type {Problem[]} */
  const errors = [...problems];
  const add = (file, field, message, fix) => errors.push({ file, field, message, fix });

  // The folder: only the known files, and a feature's notes beside its file.
  for (const name of files.keys()) {
    if (isHidden(name)) continue;
    const match = FEATURE_FILE.exec(name);
    if (match) {
      if (!ID_PATTERN.test(match[1])) add(`plan/${name}`, '-', `"${match[1]}" is not a feature id (${ID_PATTERN.source})`, 'rename the file: lowercase letters, digits and dashes');
      else if (match[2] === 'md' && !files.has(`features/${match[1]}.json`)) add(`plan/${name}`, '-', 'notes without a feature file', `create plan/features/${match[1]}.json or delete the notes`);
    } else if (!TOP_FILES.has(name)) {
      add(`plan/${name}`, '-', 'not a file of the plan', 'move it out of plan/; plan/ holds PRODUCT.md, DESIGN.md, areas.json, order.json, decisions.json and features/<id>.json and <id>.md');
    }
  }

  const unknownKeys = (file, field, object, keys) => {
    for (const key of Object.keys(object)) if (!keys.includes(key)) add(file, field ? `${field}.${key}` : key, 'unknown key', `remove it; the keys are ${keys.join(', ')}`);
  };
  const noNulls = (file, field, object) => {
    for (const [key, value] of Object.entries(object)) if (value === null) add(file, field ? `${field}.${key}` : key, 'null', 'leave an optional key out instead of null');
  };

  // areas.json
  const areaFile = 'plan/areas.json';
  const areaIds = new Set();
  if (raw.areas !== undefined) {
    if (!Array.isArray(raw.areas)) add(areaFile, '-', 'not an array', 'write an array of { id, title, line }');
    else raw.areas.forEach((area, index) => {
      const field = `[${index}]`;
      if (!isObject(area)) return add(areaFile, field, 'not an object', 'write { id, title, line }');
      unknownKeys(areaFile, field, area, AREA_KEYS);
      for (const key of AREA_KEYS) if (!isText(area[key])) add(areaFile, `${field}.${key}`, 'missing or empty', `write the area's ${key}`);
      if (typeof area.id !== 'string') return;
      if (!ID_PATTERN.test(area.id)) add(areaFile, `${field}.id`, `"${area.id}" is not an id`, 'use lowercase letters, digits and dashes');
      if (areaIds.has(area.id)) add(areaFile, `${field}.id`, `"${area.id}" appears twice`, 'give each area its own id');
      areaIds.add(area.id);
    });
  }

  // order.json
  const orderFile = 'plan/order.json';
  /** @type {Map<string, number>} */
  const placedCount = new Map();
  /** @type {Map<string, string>} */
  const stageOf = new Map();
  if (raw.order !== undefined) {
    if (!isObject(raw.order)) add(orderFile, '-', 'not an object', 'write { "stages": [...] }');
    else {
      unknownKeys(orderFile, '', raw.order, ORDER_KEYS);
      if (!Array.isArray(raw.order.stages)) add(orderFile, 'stages', 'not an array', 'write an array of { id, title, line, features }');
      else {
        const stageIds = new Set();
        raw.order.stages.forEach((stage, index) => {
          const field = `stages[${index}]`;
          if (!isObject(stage)) return add(orderFile, field, 'not an object', 'write { id, title, line, features }');
          unknownKeys(orderFile, field, stage, STAGE_KEYS);
          for (const key of ['id', 'title', 'line']) if (!isText(stage[key])) add(orderFile, `${field}.${key}`, 'missing or empty', `write the stage's ${key}`);
          if (typeof stage.id === 'string') {
            if (!ID_PATTERN.test(stage.id)) add(orderFile, `${field}.id`, `"${stage.id}" is not an id`, 'use lowercase letters, digits and dashes');
            if (stageIds.has(stage.id)) add(orderFile, `${field}.id`, `"${stage.id}" appears twice`, 'give each stage its own id');
            stageIds.add(stage.id);
            if (stage.id === GONE_STAGE && index !== raw.order.stages.length - 1) add(orderFile, `${field}.id`, `the stage "${GONE_STAGE}" is not the last`, `move "${GONE_STAGE}" to the end`);
          }
          if (!Array.isArray(stage.features)) return add(orderFile, `${field}.features`, 'not an array', 'write the feature ids in build order');
          stage.features.forEach((id, at) => {
            if (typeof id !== 'string') return add(orderFile, `${field}.features[${at}]`, 'not a feature id', 'write the id as a string');
            placedCount.set(id, (placedCount.get(id) ?? 0) + 1);
            if (!stageOf.has(id)) stageOf.set(id, stage.id);
          });
        });
      }
    }
  }
  for (const [id, count] of placedCount) {
    if (!Object.hasOwn(raw.features, id)) add(orderFile, id, 'no feature file has this id', `create plan/features/${id}.json or remove "${id}" from order.json`);
    if (count > 1) add(orderFile, id, `placed ${count} times`, 'keep it in one place only');
  }

  // features/<id>.json
  const featureIds = Object.keys(raw.features).filter((id) => ID_PATTERN.test(id));
  for (const id of Object.keys(raw.features)) {
    const file = `plan/features/${id}.json`;
    const data = raw.features[id];
    if (!isObject(data)) {
      add(file, '-', 'not an object', 'write { name, what, area, needs, ... }');
      continue;
    }
    unknownKeys(file, '', data, FEATURE_KEYS);
    noNulls(file, '', data);
    if (!isText(data.name)) add(file, 'name', 'missing or empty', 'write the feature\'s name');
    if (!isText(data.what)) add(file, 'what', 'missing or empty', 'write what the feature does, in one or two sentences');
    for (const field of ['name', 'what']) {
      if (isText(data[field]) && (data[field].includes('<!--') || data[field].includes('-->'))) add(file, field, 'holds <!-- or -->', 'remove it: the issue body would read it as a marker');
    }
    if (!placedCount.has(id)) add(file, '-', 'not in order.json', `place "${id}" in one stage of plan/order.json`);
    const stage = stageOf.get(id);
    if (data.area === undefined) {
      if (stage !== undefined && stage !== GONE_STAGE) add(file, 'area', 'missing', `name one of the areas of plan/areas.json; only the stage "${GONE_STAGE}" allows none`);
    } else if (typeof data.area !== 'string' || !areaIds.has(data.area)) {
      if (data.area !== null) add(file, 'area', `"${data.area}" is not an area`, `use one of the ids in plan/areas.json: ${[...areaIds].join(', ')}`);
    }
    if (!Array.isArray(data.needs)) add(file, 'needs', 'missing or not an array', 'write the ids of the features it needs, or []');
    else {
      const seen = new Set();
      data.needs.forEach((need, at) => {
        const field = `needs[${at}]`;
        if (typeof need !== 'string') return add(file, field, 'not a feature id', 'write the id as a string');
        if (need === id) add(file, field, 'the feature needs itself', `remove "${id}" from its needs`);
        else if (!Object.hasOwn(raw.features, need)) add(file, field, `"${need}" is not a feature`, 'name an existing feature, or remove it');
        if (seen.has(need)) add(file, field, `"${need}" appears twice`, 'keep it once');
        seen.add(need);
      });
    }
    if (data.surfaces !== undefined && data.surfaces !== null) {
      if (!isObject(data.surfaces)) add(file, 'surfaces', 'not an object', 'write { "app": "…", "cli": "…" }');
      else {
        unknownKeys(file, 'surfaces', data.surfaces, SURFACES_KEYS);
        noNulls(file, 'surfaces', data.surfaces);
        if (!Object.keys(data.surfaces).length) add(file, 'surfaces', 'empty', 'leave surfaces out when it has neither app nor cli');
        for (const key of SURFACES_KEYS) if (data.surfaces[key] !== undefined && data.surfaces[key] !== null && !isText(data.surfaces[key])) add(file, `surfaces.${key}`, 'not a text', 'write a text, or leave it out');
      }
    }
    if (data.suggest !== undefined && data.suggest !== null) {
      if (!isObject(data.suggest)) add(file, 'suggest', 'not an object', 'write { "verdict": "take", "why": "…" }');
      else {
        unknownKeys(file, 'suggest', data.suggest, SUGGEST_KEYS);
        noNulls(file, 'suggest', data.suggest);
        if (!VERDICTS.includes(data.suggest.verdict)) add(file, 'suggest.verdict', `"${data.suggest.verdict}" is not a verdict`, `use ${VERDICTS.join(', ')}`);
        if (data.suggest.why !== undefined && data.suggest.why !== null && !isText(data.suggest.why)) add(file, 'suggest.why', 'not a text', 'write a text, or leave it out');
      }
    }
    if (data.suggested_in !== undefined && data.suggested_in !== null && !(Number.isInteger(data.suggested_in) && data.suggested_in > 0)) {
      add(file, 'suggested_in', 'not an issue number', 'write the number of the suggestion issue');
    }
    if (data.verdict !== undefined && data.verdict !== null && !VERDICTS.includes(data.verdict)) add(file, 'verdict', `"${data.verdict}" is not a verdict`, `use ${VERDICTS.join(', ')}, or leave it out for undecided`);
    if (data.built !== undefined && data.built !== null) {
      if (!isDate(data.built)) add(file, 'built', `"${data.built}" is not a date`, 'write YYYY-MM-DD, UTC');
      if (data.verdict !== 'take') add(file, 'built', 'built without the verdict take', 'only a feature the owner took is built');
    }
  }

  // No cycle in needs.
  const needsOf = (id) => (Array.isArray(raw.features[id]?.needs) ? raw.features[id].needs.filter((need) => typeof need === 'string' && need !== id && Object.hasOwn(raw.features, need)) : []);
  /** @type {Map<string, 'open' | 'done'>} */
  const visit = new Map();
  const reported = new Set();
  const walk = (id, path) => {
    visit.set(id, 'open');
    for (const need of needsOf(id)) {
      if (visit.get(need) === 'open') {
        const cycle = [...path.slice(path.indexOf(need)), need];
        const key = [...new Set(cycle)].sort().join(' ');
        if (!reported.has(key)) {
          reported.add(key);
          add(`plan/features/${id}.json`, 'needs', `a cycle: ${cycle.join(' → ')}`, `remove one of these needs`);
        }
      } else if (!visit.has(need)) walk(need, [...path, need]);
    }
    visit.set(id, 'done');
  };
  for (const id of featureIds.sort()) if (!visit.has(id)) walk(id, [id]);

  // decisions.json
  const decisionFile = 'plan/decisions.json';
  if (raw.decisions !== undefined) {
    if (!Array.isArray(raw.decisions)) add(decisionFile, '-', 'not an array', 'write an array of { id, date, text, about }');
    else {
      const ids = new Set();
      raw.decisions.forEach((decision, index) => {
        const field = `[${index}]`;
        if (!isObject(decision)) return add(decisionFile, field, 'not an object', 'write { id, date, text, about }');
        unknownKeys(decisionFile, field, decision, DECISION_KEYS);
        noNulls(decisionFile, field, decision);
        if (typeof decision.id !== 'string' || !ID_PATTERN.test(decision.id)) add(decisionFile, `${field}.id`, `"${decision.id}" is not an id`, 'use lowercase letters, digits and dashes');
        else if (ids.has(decision.id)) add(decisionFile, `${field}.id`, `"${decision.id}" appears twice`, 'give each decision its own id');
        ids.add(decision.id);
        if (!isDate(decision.date)) add(decisionFile, `${field}.date`, `"${decision.date}" is not a date`, 'write YYYY-MM-DD');
        if (!isText(decision.text)) add(decisionFile, `${field}.text`, 'missing or empty', 'write what was decided');
        if (decision.about !== undefined && decision.about !== null) {
          if (!Array.isArray(decision.about) || !decision.about.length) add(decisionFile, `${field}.about`, 'not a list of area ids', 'name areas, or leave about out for the whole product');
          else {
            const named = new Set();
            decision.about.forEach((target, at) => {
              const where = `${field}.about[${at}]`;
              if (typeof target === 'string' && Object.hasOwn(raw.features, target) && !areaIds.has(target)) add(decisionFile, where, `"${target}" is a feature, not an area`, `a decision about one feature goes in plan/features/${target}.md; about names areas only`);
              else if (!areaIds.has(target)) add(decisionFile, where, `"${target}" is not an area`, `use one of the ids in plan/areas.json: ${[...areaIds].join(', ')}`);
              if (named.has(target)) add(decisionFile, where, `"${target}" appears twice`, 'keep it once');
              named.add(target);
            });
          }
        }
      });
    }
  }

  // Canonical form, for every JSON file that parsed.
  const canonical = (name, value, form) => {
    if (value === undefined) return;
    if (files.get(name) !== canonicalText(form(value))) add(`plan/${name}`, '-', 'not in canonical form', 'write it as JSON.stringify(value, null, 2) plus a final newline, keys in the order of tools/plan/README.md');
  };
  canonical(PLAN_FILES.areas, raw.areas, canonicalAreas);
  canonical(PLAN_FILES.order, raw.order, canonicalOrder);
  canonical(PLAN_FILES.decisions, raw.decisions, canonicalDecisions);
  for (const id of Object.keys(raw.features)) canonical(`features/${id}.json`, raw.features[id], canonicalFeature);

  /** @type {string[]} */
  const notes = [];
  if (knownAreas) {
    for (const id of areaIds) {
      if (!knownAreas.includes(id)) notes.push(`plan/areas.json: the area "${id}" is new. Draw its place on the map in website/src/workshop/map.ts; the site build checks it.`);
    }
  }
  return { errors, notes };
}
