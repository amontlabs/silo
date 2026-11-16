// The Stack view: the feature list one entry at a time, the stages drawn as a
// stack, and the product drawn as it grows. Pure functions from data to HTML,
// like views.js. Every text from features.jsonl goes through h`` (escaped);
// the verdict buttons exist only inside the [data-controls] wrapper of their
// own entry, which carries the newest verdict row the page showed. `what`
// and `why` are inline Markdown through the page's one renderer, which
// escapes raw HTML.
import { h, raw } from './html.js';
import { renderInline } from './markdown.js';
import { STARTER_LAST_STAGE, appWords, currentVerdicts, nextUndecided, schematicParts, stackTotals } from './features.js';

export const VERDICT_CHIP = { take: 'Taken', refine: 'Refine', later: 'Later', drop: 'Dropped' };
const VERDICT_BUTTON = { take: 'Take', refine: 'Refine…', later: 'Later', drop: 'Drop' };
const SUGGEST_WORD = { take: 'Take', refine: 'Refine', later: 'Later', drop: 'Drop' };
const OLD_WORDS = {
  driven: 'worked in a live run',
  partial: 'worked with gaps',
  built: 'built, never proven',
  planned: 'planned',
  idea: 'an idea',
  dropped: 'dropped',
};
const REUSE_WORDS = { reuse: 'carry over', rework: 'right idea, clean up', rewrite: 'start again' };
const FILTERS = [
  ['all', 'All'],
  ['undecided', 'Undecided'],
  ['taken', 'Taken'],
  ['later', 'Later'],
  ['dropped', 'Dropped'],
];

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/**
 * The stack's data, ready for the page, or null when there is no feature
 * list. `stack` = { features, stages, skipped, rows (newest verdict row per
 * feature) }.
 */
export function prepareStack(stack) {
  if (!stack) return null;
  const { features, stages, skipped } = stack;
  const verdicts = currentVerdicts(features, stack.rows ?? []);
  const start = nextUndecided(features, verdicts) ?? features[0] ?? null;
  return {
    features,
    stages,
    skipped,
    verdicts,
    totals: stackTotals(features, verdicts),
    start: start?.id ?? null,
    stageTitle: new Map(stages.map((stage) => [stage.id, stage.title])),
  };
}

const verdictOf = (prep, feature) => prep.verdicts.get(feature.id)?.verdict ?? null;
const vClass = (verdict) => `v-${verdict ?? 'none'}`;

function totalsLine(counts) {
  const parts = [
    ['take', `${counts.take} taken`],
    ['refine', `${counts.refine} refine`],
    ['later', `${counts.later} later`],
    ['drop', `${counts.drop} dropped`],
    ['none', `${counts.undecided} undecided`],
  ];
  return h`<p class="totals">${parts.map(([key, words], index) => h`${index ? ' · ' : ''}<span class="t ${vClass(key === 'none' ? null : key)}">${words}</span>`)}</p>`;
}

// ---- the stack of stages ----

function tower(prep) {
  const tiers = [];
  for (const stage of prep.stages) {
    const counts = prep.totals.stages.get(stage.id);
    const entries = prep.features.filter((feature) => feature.stage === stage.id);
    tiers.push(h`<li><button class="tier" type="button" data-stage-jump="${stage.id}" data-stage="${stage.id}" title="${stage.line}">
      <span class="tier-title">${stage.title}</span><span class="tier-count">${counts.total - counts.undecided}/${counts.total}</span>
      <span class="segs" aria-hidden="true">${entries.map((feature) => h`<i class="${vClass(verdictOf(prep, feature))}"></i>`)}</span>
    </button></li>`);
    if (stage.id === STARTER_LAST_STAGE && stage !== prep.stages.at(-1)) tiers.push(h`<li class="cut"><span>Starter: you can use it daily</span></li>`);
  }
  return h`<nav class="tower" aria-label="Stages, the first at the bottom"><ol>${tiers}</ol></nav>`;
}

// ---- one entry ----

function oldCell(feature) {
  const extra = [];
  if (feature.old_lines > 0) extra.push(`about ${feature.old_lines.toLocaleString('en-US')} lines`);
  if (REUSE_WORDS[feature.reuse]) extra.push(REUSE_WORDS[feature.reuse]);
  return h`<div class="fact"><span class="fact-label">In the old code</span>
    <p><span class="pip old-${feature.old ?? 'none'}" aria-hidden="true"></span>${feature.old ? OLD_WORDS[feature.old] : 'not recorded'}</p>
    ${extra.length ? h`<small>${extra.join(' · ')}</small>` : ''}
  </div>`;
}

function suggestCell(feature) {
  return h`<div class="fact"><span class="fact-label">Suggested</span>
    <p class="sugg ${vClass(feature.suggest)}">${feature.suggest ? SUGGEST_WORD[feature.suggest] : 'No suggestion'}</p>
    ${feature.why ? h`<small>${raw(renderInline(feature.why))}</small>` : ''}
  </div>`;
}

function showsUp(feature) {
  const words = appWords(feature.app);
  if (!words && !feature.cli) return '';
  return h`<div class="shows"><span class="fact-label">Shows up as</span>
    ${words ? h`<p>${words[0].toUpperCase()}${words.slice(1)}</p>` : ''}
    ${feature.cli ? h`<code>${feature.cli}</code>` : ''}
  </div>`;
}

function entryControls(prep, feature) {
  const current = prep.verdicts.get(feature.id);
  const verdict = current?.verdict ?? null;
  const primary = feature.suggest ?? 'take';
  const buttons = Object.entries(VERDICT_BUTTON).map(
    ([value, label]) =>
      h`<button class="btn ${value === primary ? 'go' : ''}" type="button" data-pick="${value}" ${value === primary ? h`data-primary` : ''}>${label}${value === primary ? h` <kbd>↵</kbd>` : ''}</button>`,
  );
  const now = verdict
    ? h`<div class="verdict-now"><span class="vchip ${vClass(verdict)}">${VERDICT_CHIP[verdict]}</span>${current.source === 'file' ? h`<span class="v-from">from the list</span>` : ''}
        ${current.note ? h`<p class="v-note">${current.note}</p>` : ''}
        <span class="v-acts"><button class="btn small" type="button" data-change aria-expanded="false">Change</button><button class="btn ghost small" type="button" data-pick="clear">Clear</button></span></div>`
    : '';
  return h`<div class="controls ${verdict ? 'decided' : ''}" data-controls data-feature="${feature.id}" data-shown="${current?.shown ?? 0}">
    ${now}
    <div class="acts verdict-acts">${buttons}<button class="btn ghost small" type="button" data-say aria-expanded="false">Add a note</button><button class="btn ghost skip" type="button" data-stack-skip>Skip <kbd>→</kbd></button></div>
    <form class="say" data-feature-note data-mode="note" data-draft="feature-${feature.id}-note">
      <textarea name="text" rows="3" placeholder="A note for this entry" aria-label="A note for this entry"></textarea>
      <div class="row"><span class="note-hint">The note goes with the verdict you pick.</span><span class="refine-hint">Refine needs a note: what to change.</span><button class="btn go" type="submit">Send as Refine <kbd>⌘↵</kbd></button></div>
    </form>
    <p class="form-error" role="alert"></p>
  </div>`;
}

function entryCard(prep, feature) {
  const verdict = verdictOf(prep, feature);
  const total = prep.features.length;
  return h`<article class="card entry" data-entry="${feature.id}" data-stage="${feature.stage}" data-decided="${verdict ?? ''}" ${feature.id === prep.start ? h`data-start` : ''} tabindex="-1" aria-label="${feature.n} of ${total}: ${feature.name}">
    <div class="card-top"><span class="kind stage-pill">${prep.stageTitle.get(feature.stage) ?? feature.stage}</span><span class="where">${feature.n} of ${total}</span></div>
    <h2>${feature.name}</h2>
    ${feature.what ? h`<p class="body">${raw(renderInline(feature.what))}</p>` : ''}
    <div class="facts">${oldCell(feature)}${suggestCell(feature)}</div>
    ${showsUp(feature)}
    ${entryControls(prep, feature)}
  </article>`;
}

// ---- lists: coming next, see all ----

function entryRow(prep, feature, { full }) {
  const verdict = verdictOf(prep, feature);
  return h`<li data-v="${verdict ?? 'none'}" data-row="${feature.id}"><button class="row" type="button" data-entry-focus="${feature.id}">
    <span class="row-n">${feature.n}</span>
    ${full ? (verdict ? h`<span class="vchip small ${vClass(verdict)}">${VERDICT_CHIP[verdict]}</span>` : h`<span class="vchip small v-none">undecided</span>`) : ''}
    <span class="row-title">${feature.name}</span>
    ${full ? h`<span class="pip old-${feature.old ?? 'none'}" title="In the old code: ${feature.old ? OLD_WORDS[feature.old] : 'not recorded'}"></span>` : ''}
    <span class="sugg-word ${vClass(feature.suggest)}">${feature.suggest ? SUGGEST_WORD[feature.suggest].toLowerCase() : ''}</span>
  </button></li>`;
}

function comingNext(prep) {
  return h`<div class="coming"><p class="eyebrow">Coming next</p><ol class="rows">${prep.features.map((feature) => entryRow(prep, feature, { full: false }))}</ol></div>`;
}

function seeAll(prep) {
  return h`<div class="stack-all" data-filter="all">
    <div class="stack-filters" role="group" aria-label="Show">${FILTERS.map(
      ([value, label]) => h`<button class="pill ${value === 'all' ? 'on' : ''}" type="button" data-stack-filter="${value}" aria-pressed="${value === 'all' ? 'true' : 'false'}">${label}</button>`,
    )}</div>
    ${prep.stages.map((stage) => {
      const counts = prep.totals.stages.get(stage.id);
      const entries = prep.features.filter((feature) => feature.stage === stage.id);
      return h`<details class="stack-group" id="stack-group-${stage.id}" data-stage="${stage.id}">
        <summary><b>${stage.title}</b><span>${counts.total - counts.undecided}/${counts.total} decided</span></summary>
        <ol class="rows">${entries.map((feature) => entryRow(prep, feature, { full: true }))}</ol>
      </details>`;
    })}
  </div>`;
}

// ---- the product so far ----

function partClass(part) {
  return `part${part.taken ? ' is-taken' : ''}${part.state === 'current' ? ' is-current' : ''}`;
}

function soFar(prep) {
  const drawing = schematicParts(prep.features, prep.verdicts, prep.start);
  const { parts } = drawing;
  const of = (kind) => parts.filter((part) => part.kind === kind);
  const attrs = (part) => h`class="${partClass(part)}" data-part="${part.key}" data-features="${part.features.join(' ')}"`;
  const one = (kind, body) => of(kind).map((part) => h`<span ${attrs(part)}>${body}</span>`);
  const windowParts = of('window');
  return h`<aside class="so-far" aria-label="The product so far">
    <p class="eyebrow">The product so far</p>
    <div class="mac">
      <div class="menubar">${one('menubar', h`<i aria-hidden="true"></i>`)}</div>
      <div class="win ${windowParts.some((part) => part.taken) ? 'has-window' : ''}">
        <div class="win-bar" aria-hidden="true"><i></i><i></i><i></i></div>
        <div class="win-body">
          <div class="win-side">${of('side').map((part) => h`<span ${attrs(part)}>${part.label}</span>`)}</div>
          <div class="win-list">${windowParts.map((part) => h`<span ${attrs(part)}><b>main</b><b>fix-login</b></span>`)}${one('tree', h`<b>try-redis</b>`)}</div>
          <div class="win-page">
            <div class="win-buttons">${of('button').map((part) => h`<span ${attrs(part)}>${part.label}</span>`)}</div>
            <div class="win-tabs">${of('tab').map((part) => h`<span ${attrs(part)}>${part.label}</span>`)}</div>
          </div>
        </div>
        <div class="win-foot">${one('progress', h`<i aria-hidden="true"></i>`)}</div>
      </div>
    </div>
    ${drawing.empty ? h`<p class="so-far-hint">Nothing taken yet. Take an entry and its part of the app appears here.</p>` : ''}
    <div class="term"><div class="term-bar" aria-hidden="true"><i></i><i></i><i></i><span>Terminal</span></div>
      <ul>${drawing.cli.map((line) => h`<li class="${partClass(line)}" data-features="${line.feature}"><span aria-hidden="true">$ </span>${line.text}</li>`)}</ul>
      <p class="term-empty">No commands yet</p>
    </div>
  </aside>`;
}

// ---- the view ----

export function stackView(prep) {
  if (!prep) return '';
  const { all } = prep.totals;
  const warning = prep.skipped
    ? h`<p class="stack-warn" role="note">${plural(prep.skipped, 'line')} of features.jsonl could not be read and ${prep.skipped === 1 ? 'is' : 'are'} not shown.</p>`
    : '';
  return h`<section class="view" id="stack" data-region="stack" aria-label="Stack">
  <div class="stack-head">
    <p class="eyebrow">The stack</p>
    <p class="lede">Every feature, starter first</p>
    ${totalsLine(all)}
    ${warning}
  </div>
  <div class="stack-layout" data-stack>
    ${tower(prep)}
    <div class="stack-main">
      <div class="stack-tools"><p class="stack-done" ${all.undecided ? h`hidden` : ''}>Every entry has a verdict. Change any of them below.</p><span class="spacer"></span><button class="btn ghost small" type="button" data-stack-all aria-pressed="false">See all</button></div>
      <div class="stack-cards" aria-live="polite">${prep.features.map((feature) => entryCard(prep, feature))}</div>
      <div class="hints" aria-hidden="true"><span><kbd>↵</kbd> the suggestion</span><span><kbd>→</kbd> skip</span><span><kbd>←</kbd> back</span><span><kbd>⇥</kbd> add a note</span></div>
      ${comingNext(prep)}
      ${seeAll(prep)}
    </div>
    ${soFar(prep)}
  </div>
</section>`;
}

// The line in For you's side column.
export function stackLink(prep) {
  if (!prep) return '';
  const left = prep.totals.all.undecided;
  return h`<a class="mini stack-mini" href="#stack"><span class="mini-stack" aria-hidden="true"><i></i><i></i><i></i></span><span><b>The stack</b><small>${left ? `${plural(left, 'entry', 'entries')} without a verdict` : 'every entry has a verdict'}</small></span></a>`;
}
