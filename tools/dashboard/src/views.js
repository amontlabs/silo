// The page: pure functions from data to HTML. Section regions carry
// data-region so the browser can refresh one at a time.
import { h, raw, join, esc } from './html.js';
import { lexMarkdown, renderMarkdown } from './markdown.js';
import { countListItems, DOC_FILES, STEP_STATUSES, lineDiff, parseJourneys, slugify, splitSections } from './docs.js';
import { forYouOrder, lastActivity, lastReplyId } from './state.js';

const KIND_LABEL = { decide: 'Decide', look: 'Look', shipped: 'Shipped' };
const KIND_HINT = {
  decide: 'Agents could not pick: this needs you',
  look: 'An agent made a reversible choice',
  shipped: 'What a build delivered',
};
const STATE_LABEL = { waiting: 'Waiting for you', answered: 'Answered', closed: 'Closed' };

function time(iso) {
  if (!iso) return '';
  const date = new Date(iso);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  const text = date.toLocaleString('en-GB', {
    day: 'numeric',
    month: 'short',
    ...(sameYear ? {} : { year: 'numeric' }),
    hour: '2-digit',
    minute: '2-digit',
  });
  return h`<time datetime="${iso}" title="${iso}">${text}</time>`;
}

const chip = (kind, label, title) => h`<span class="chip ${kind}" ${title ? raw(`title="${esc(title)}"`) : ''}>${label}</span>`;

// ---- items ----

function attachmentView(attachment) {
  const url = `/attachments/${attachment.id}/${encodeURIComponent(attachment.file)}`;
  const label = attachment.caption || attachment.file;
  const type = attachment.media_type;
  let media;
  if (type.startsWith('video/')) media = h`<video controls preload="metadata" src="${url}"></video>`;
  else if (type.startsWith('image/')) media = h`<a href="${url}" target="_blank" rel="noopener"><img loading="lazy" src="${url}" alt="${label}"></a>`;
  else if (type.startsWith('audio/')) media = h`<audio controls preload="metadata" src="${url}"></audio>`;
  else media = h`<a class="file-link" href="${url}" download="${attachment.file}">${attachment.file} <small>${type}</small></a>`;
  return h`<figure class="attachment">${media}${attachment.caption ? h`<figcaption>${attachment.caption}</figcaption>` : ''}</figure>`;
}

function replyView(reply) {
  const mine = reply.author === 'owner';
  return h`<li class="reply ${mine ? 'reply-owner' : 'reply-agent'}">
    <div class="reply-meta">
      <strong>${mine ? 'You' : 'Agent'}</strong>
      ${reply.verdict ? chip(reply.verdict, reply.verdict === 'agree' ? 'Agreed' : 'Disagreed') : ''}
      ${time(reply.created_at)}
      ${mine && reply.acked_at ? h`<span class="muted">· read by an agent</span>` : ''}
    </div>
    ${reply.text ? h`<div class="prose">${raw(renderMarkdown(reply.text))}</div>` : ''}
  </li>`;
}

function replyForm(item, { verdict = null, label, placeholder, required = true, primary = false, rows = 3 }) {
  return h`<form class="reply-form" data-api="/api/items/${item.id}/replies" ${verdict ? raw(`data-verdict="${verdict}"`) : ''}>
    <textarea name="text" rows="${rows}" ${required ? 'required' : ''} placeholder="${placeholder}" aria-label="${placeholder}"></textarea>
    <div class="form-row"><button class="btn ${primary ? 'primary' : ''}" type="submit">${label}</button><span class="form-error" role="alert"></span></div>
  </form>`;
}

// Every owner control sits in a [data-controls] wrapper that carries the item
// and the newest reply the page showed; the server refuses an action when the
// item has a newer reply, so the owner never answers something unread.
function actionsView(item) {
  if (item.state === 'closed') return '';
  const wrap = (content, extra = '') =>
    h`<div class="actions ${extra}" data-controls data-item="${item.id}" data-after="${lastReplyId(item)}">${content}<span class="form-error" role="alert"></span></div>`;
  const comment = h`<details class="reply-box"><summary class="btn">Comment</summary>${replyForm(item, { label: 'Send comment', placeholder: 'Your comment' })}</details>`;
  if (item.state !== 'waiting') return wrap(comment);
  if (item.kind === 'decide') {
    return wrap(h`<label class="field-label">Your answer</label>${replyForm(item, { label: 'Send answer', placeholder: 'Your answer', primary: true, rows: 4 })}`, 'actions-form');
  }
  if (item.kind === 'look') {
    return wrap(h`
      <button class="btn primary" type="button" data-post="/api/items/${item.id}/replies" data-body='{"verdict":"agree"}'>Agree</button>
      <details class="reply-box"><summary class="btn">Disagree</summary>${replyForm(item, { verdict: 'disagree', label: 'Send disagreement', placeholder: 'What should happen instead?' })}</details>
      ${comment}`);
  }
  return wrap(h`
    <button class="btn primary" type="button" data-post="/api/items/${item.id}/seen" data-body="{}">Seen</button>
    ${comment}`);
}

function itemHeader(item, journeys) {
  const known = journeys.get(item.journey);
  return h`<div class="item-chips">
    ${chip(`kind-${item.kind}`, KIND_LABEL[item.kind], KIND_HINT[item.kind])}
    ${item.state !== 'waiting' ? chip(`state-${item.state}`, STATE_LABEL[item.state]) : ''}
    ${item.journey ? (known ? h`<a class="chip journey" href="#journey-${item.journey}">${known}</a>` : chip('journey', item.journey)) : ''}
    ${item.source ? h`<span class="muted source">${item.source}</span>` : ''}
    <span class="muted">${time(item.created_at)} · #${item.id}</span>
  </div>`;
}

function itemBody(item, { actions }) {
  return h`
    ${item.body ? h`<div class="prose item-text">${raw(renderMarkdown(item.body))}</div>` : ''}
    ${item.default_text ? h`<div class="callout"><strong>Stands unless you say otherwise</strong><div class="prose">${raw(renderMarkdown(item.default_text))}</div></div>` : ''}
    ${item.attachments.length ? h`<div class="attachments">${item.attachments.map(attachmentView)}</div>` : ''}
    ${item.replies.length ? h`<ol class="thread">${item.replies.map(replyView)}</ol>` : ''}
    ${item.status === 'closed' ? h`<p class="muted closed-note">Closed ${time(item.closed_at)}${item.close_note ? h`: ${item.close_note}` : ''}</p>` : ''}
    ${actions ? actionsView(item) : ''}`;
}

// idPrefix keeps DOM ids unique where one item shows in two places.
function itemCard(item, journeys, { idPrefix = 'item', collapsed = false, actions = true } = {}) {
  const id = `${idPrefix}-${item.id}`;
  if (collapsed) {
    return h`<details class="item item-${item.kind} is-${item.state}" id="${id}">
      <summary>
        <span class="summary-main">${chip(`kind-${item.kind}`, KIND_LABEL[item.kind])}<span class="item-title">${item.title}</span></span>
        <span class="summary-side">${chip(`state-${item.state}`, STATE_LABEL[item.state])}<span class="muted">${time(lastActivity(item))}</span></span>
      </summary>
      <div class="item-inner">${itemHeader(item, journeys)}${itemBody(item, { actions })}</div>
    </details>`;
  }
  return h`<article class="item item-${item.kind} is-${item.state}" id="${id}">
    ${itemHeader(item, journeys)}
    <h3 class="item-title">${item.title}</h3>
    ${itemBody(item, { actions })}
  </article>`;
}

// ---- document changes ----

function diffView(change) {
  const rows = lineDiff(change.before, change.after);
  const keep = new Set();
  rows.forEach((row, index) => {
    if (row.op === ' ') return;
    for (let n = Math.max(0, index - 2); n <= Math.min(rows.length - 1, index + 2); n += 1) keep.add(n);
  });
  const out = [];
  let skipped = 0;
  rows.forEach((row, index) => {
    if (!keep.has(index)) {
      skipped += 1;
      return;
    }
    if (skipped) out.push(h`<div class="diff-gap">${skipped} unchanged line${skipped === 1 ? '' : 's'}</div>`);
    skipped = 0;
    out.push(h`<div class="diff-line diff-${row.op === '+' ? 'add' : row.op === '-' ? 'del' : 'ctx'}"><span class="diff-op">${row.op === ' ' ? '' : row.op}</span><span>${row.line || ' '}</span></div>`);
  });
  if (skipped) out.push(h`<div class="diff-gap">${skipped} unchanged line${skipped === 1 ? '' : 's'}</div>`);
  return h`<div class="diff" role="group" aria-label="Before and after">${out}</div>`;
}

const CHANGE_LABEL = { changed: 'Changed', new: 'New', removed: 'Removed' };

function changeMeta(change, id) {
  if (!change) return '';
  return h`<div class="doc-meta">
    ${chip(`change-${change.status}`, CHANGE_LABEL[change.status])}
    ${change.status === 'changed' || change.status === 'removed'
      ? h`<details class="diff-box" id="diff-${id}"><summary>Show what changed</summary>${diffView(change)}</details>`
      : h`<span class="muted">Not seen yet</span>`}
  </div>`;
}

// ---- sections ----

function emptyState(text) {
  return h`<p class="empty">${text}</p>`;
}

function changeIndex(changes) {
  const map = new Map();
  for (const change of changes) map.set(`${change.file}#${change.key}`, change);
  return map;
}

function forYouSection(waiting, view) {
  const { changes, state } = view.changes;
  const live = changes;
  const list = (rows) => h`<ul class="change-list">${rows.map((change) => {
    const label = h`${chip(`change-${change.status}`, CHANGE_LABEL[change.status])} <span class="muted">${change.file}</span> ${change.heading || 'Introduction'}`;
    const target = change.status === 'removed' ? null : anchorFor(change, view);
    return h`<li>${target ? h`<a href="${target}">${label}</a>` : label}</li>`;
  })}</ul>`;
  let docs;
  if (state === 'nogit') docs = '';
  else if (!live.length) docs = h`<div class="docs-changes quiet"><p class="muted">No changes in the product documents since you last looked.</p></div>`;
  else {
    const headline =
      state === 'tracked'
        ? `${live.length} section${live.length === 1 ? '' : 's'} of the product documents changed since you last looked.`
        : state === 'never'
          ? `You have not marked the product documents as seen yet: ${live.length} section${live.length === 1 ? '' : 's'}.`
          : `The commit you last marked is no longer in this checkout: ${live.length} section${live.length === 1 ? '' : 's'} count as new.`;
    docs = h`<details class="docs-changes" ${state === 'tracked' ? 'open' : ''} id="docs-changes">
      <summary><span class="summary-main"><strong>Changed since you last looked</strong></span><span class="muted">${headline}</span></summary>
      ${list(live)}
    </details>`;
  }
  const head = view.changes.head;
  const markSeen =
    state === 'nogit' || !head
      ? ''
      : h`<span class="actions" data-controls><button class="btn" type="button" data-post="/api/docs/seen" data-body="${JSON.stringify({ commit: head })}" data-refresh="all">Mark all as seen</button><span class="form-error" role="alert"></span></span>`;
  return h`<section id="for-you" class="panel" data-region="for-you">
    <div class="section-head"><h2>For you</h2>${markSeen}</div>
    ${docs}
    ${waiting.length ? join(waiting.map((item) => itemCard(item, view.journeyTitles))) : emptyState('Nothing is waiting for you.')}
  </section>`;
}

function anchorFor(change, view) {
  if (change.file === DOC_FILES.product) return change.key ? `#product-${change.key}` : '#product';
  if (change.file === DOC_FILES.decisions) return change.key ? `#decisions-${change.key}` : '#decisions';
  const journey = view.journeys.journeys.find((entry) => entry.key === change.key);
  return journey ? `#journey-${journey.id}` : '#journeys';
}

function docSections(text, file, prefix, changes, extra = {}) {
  const { preamble, sections } = splitSections(text);
  const index = changeIndex(changes);
  const render = extra.render || renderMarkdown;
  const intro = preamble.trim()
    ? h`<article class="doc-section" ${raw(`id="${prefix}-intro"`)}>${changeMeta(index.get(`${file}#`), `${prefix}-intro`)}<div class="prose">${raw(render(preamble))}</div></article>`
    : '';
  return h`${intro}${sections.map((section) => h`<article class="doc-section" id="${prefix}-${section.key}">
    ${changeMeta(index.get(`${file}#${section.key}`), `${prefix}-${section.key}`)}
    <div class="prose">${raw(render(section.text))}</div>
  </article>`)}`;
}

function removedNotes(changes, file) {
  const gone = changes.filter((change) => change.file === file && change.status === 'removed');
  if (!gone.length) return '';
  return h`<div class="removed-notes">${gone.map((change) => h`<div class="doc-meta">${chip('change-removed', 'Removed')} <span>${change.heading || 'Introduction'}</span>
    <details class="diff-box" id="diff-removed-${file}-${change.key}"><summary>Show what was removed</summary>${diffView(change)}</details></div>`)}</div>`;
}

function productSection(view) {
  const text = view.docs.product;
  return h`<section id="product" class="panel" data-region="product">
    <div class="section-head"><h2>Product</h2><span class="muted">docs/product/${DOC_FILES.product}</span></div>
    ${text === null ? emptyState('No product description yet (docs/product/PRODUCT.md is missing).') : docSections(text, DOC_FILES.product, 'product', view.changes.changes)}
    ${removedNotes(view.changes.changes, DOC_FILES.product)}
  </section>`;
}

function progressView(counts) {
  const total = STEP_STATUSES.reduce((sum, name) => sum + counts[name], 0);
  if (!total) return h`<span class="muted">No steps</span>`;
  const seg = (name) => (counts[name] ? h`<span class="seg seg-${name}" style="flex:${counts[name]}" title="${counts[name]} ${name}"></span>` : '');
  const label = STEP_STATUSES.map((name) => `${counts[name]} ${name}`).join(', ');
  return h`<div class="progress" role="img" aria-label="${label}">${STEP_STATUSES.map(seg)}</div>
    <div class="counts">${STEP_STATUSES.map((name) => h`<span class="count"><span class="chip step-${name}">${name}</span> ${counts[name]}</span>`)}</div>`;
}

function journeyItems(items, view) {
  if (!items.length) return '';
  const waiting = items.filter((item) => item.state === 'waiting').sort(forYouOrder);
  const rest = items.filter((item) => item.state !== 'waiting').sort((a, b) => (lastActivity(a) < lastActivity(b) ? 1 : -1));
  return h`<div class="journey-items">
    ${waiting.length ? h`<ul class="waiting-rows">${waiting.map((item) => h`<li><a href="#item-${item.id}">${chip(`kind-${item.kind}`, KIND_LABEL[item.kind])} <span>${item.title}</span> <span class="muted">waiting for you</span></a></li>`)}</ul>` : ''}
    ${rest.map((item) => itemCard(item, view.journeyTitles, { idPrefix: 'jitem', collapsed: true }))}
  </div>`;
}

function journeyCard({ id, title, body, counts, key }, items, view) {
  const change = changeIndex(view.changes.changes).get(`${DOC_FILES.journeys}#${key}`);
  return h`<article class="journey-card" id="journey-${id}">
    <header>
      <h3>${title}</h3>
      <code class="journey-id" title="Journey id for --journey">${id}</code>
    </header>
    ${changeMeta(change, `journey-${id}`)}
    ${progressView(counts)}
    ${body ? h`<div class="prose">${raw(renderMarkdown(body, { steps: true }))}</div>` : ''}
    ${journeyItems(items, view)}
  </article>`;
}

function journeysSection(view) {
  const { preamble, journeys } = view.journeys;
  // The section has its own heading, so the file's leading `# ` title is not repeated.
  const intro = preamble.replace(/^\s*# [^\n]*\n?/, '');
  const known = new Set(journeys.map((journey) => journey.id));
  const byJourney = new Map();
  const other = [];
  for (const item of view.items) {
    if (item.journey && known.has(item.journey)) {
      if (!byJourney.has(item.journey)) byJourney.set(item.journey, []);
      byJourney.get(item.journey).push(item);
    } else other.push(item);
  }
  return h`<section id="journeys" class="panel" data-region="journeys">
    <div class="section-head"><h2>Journeys</h2><span class="muted">docs/product/${DOC_FILES.journeys}</span></div>
    ${view.docs.journeys === null ? emptyState('No journeys yet (docs/product/JOURNEYS.md is missing).') : ''}
    ${intro.trim() ? h`<div class="prose journeys-intro">${raw(renderMarkdown(intro))}</div>` : ''}
    <div class="journey-grid">${journeys.map((journey) => journeyCard(journey, byJourney.get(journey.id) || [], view))}</div>
    ${other.length ? h`<article class="journey-card" id="journey-other"><header><h3>Other</h3><span class="muted">items without a known journey</span></header>${journeyItems(other, view)}</article>` : ''}
    ${removedNotes(view.changes.changes, DOC_FILES.journeys)}
  </section>`;
}

function decisionsSection(view) {
  const text = view.docs.decisions;
  const total = text === null ? 0 : countListItems(lexMarkdown(text));
  return h`<section id="decisions" class="panel" data-region="decisions">
    <div class="section-head"><h2>Decisions</h2><span class="muted">docs/product/${DOC_FILES.decisions}</span></div>
    ${text === null ? emptyState('No decisions recorded yet (docs/product/DECISIONS.md is missing).') : h`
      <div class="filter-row">
        <input type="search" id="decision-filter" placeholder="Filter decisions" aria-label="Filter decisions" autocomplete="off">
        <span class="muted" id="decision-count" data-total="${total}">${total} decision${total === 1 ? '' : 's'}</span>
      </div>
      <div id="decision-list">${docSections(text, DOC_FILES.decisions, 'decisions', view.changes.changes)}</div>
      ${removedNotes(view.changes.changes, DOC_FILES.decisions)}`}
  </section>`;
}

function privateSection(groups) {
  if (!groups.length) return '';
  return h`<section id="private" class="panel" data-region="private">
    <div class="section-head"><h2>Private notes</h2><span class="muted">$SILO_DASHBOARD_HOME/private, not in the repository</span></div>
    ${groups.map((group) => h`<div class="note-group">
      ${group.folder ? h`<h3 class="note-folder">${group.folder}/</h3>` : ''}
      ${group.notes.map((note) => h`<article class="doc-section note" id="note-${slugify(note.path) || 'note'}">
        <div class="doc-meta"><code class="note-path">${note.path}</code></div>
        <div class="prose">${raw(renderMarkdown(note.text))}</div>
      </article>`)}
    </div>`)}
  </section>`;
}

function historySection(items, view) {
  return h`<section id="history" class="panel" data-region="history">
    <div class="section-head"><h2>History</h2><span class="muted">answered and closed items</span></div>
    ${items.length ? join(items.map((item) => itemCard(item, view.journeyTitles, { idPrefix: 'hist', collapsed: true }))) : emptyState('Nothing answered or closed yet.')}
  </section>`;
}

function navView(counts) {
  const link = (href, label, count, hot) => h`<a href="${href}">${label}${count === null ? '' : h`<span class="count-badge ${hot && count ? 'hot' : ''}">${count}</span>`}</a>`;
  return h`<nav id="nav" data-region="nav" aria-label="Sections">${link('#for-you', 'For you', counts.forYou, true)}${link('#product', 'Product', null)}${link('#journeys', 'Journeys', counts.journeys)}${link('#decisions', 'Decisions', counts.decisions)}${counts.notes ? link('#private', 'Private', counts.notes) : ''}${link('#history', 'History', counts.history)}</nav>`;
}

/**
 * view = { items (with state), docs: {product, journeys, decisions} (text or null), changes: docChanges result,
 *   notes: readPrivateNotes result (optional) }
 */
export function renderPage(view) {
  const journeys = parseJourneys(view.docs.journeys ?? '');
  const journeyTitles = new Map(journeys.journeys.map((journey) => [journey.id, journey.title]));
  const full = { ...view, journeys, journeyTitles };
  const waiting = view.items.filter((item) => item.state === 'waiting').sort(forYouOrder);
  const done = view.items.filter((item) => item.state !== 'waiting').sort((a, b) => (lastActivity(a) < lastActivity(b) ? 1 : -1));
  const decisionCount = view.docs.decisions === null ? 0 : countListItems(lexMarkdown(view.docs.decisions));
  const notes = view.notes ?? [];
  const noteCount = notes.reduce((sum, group) => sum + group.notes.length, 0);
  const counts = { forYou: waiting.length, journeys: journeys.journeys.length, decisions: decisionCount, notes: noteCount, history: done.length };
  return h`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>Silo owner dashboard</title>
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='8' fill='%23355cff'/%3E%3Ccircle cx='16' cy='16' r='6' fill='white'/%3E%3C/svg%3E">
<link rel="stylesheet" href="/app.css">
<script type="module" src="/app.js"></script>
</head>
<body>
<header class="topbar"><div class="topbar-inner"><a class="brand" href="#top">Silo <span>owner</span></a>${navView(counts)}</div></header>
<main id="top">
${forYouSection(waiting, full)}
${productSection(full)}
${journeysSection(full)}
${decisionsSection(full)}
${privateSection(notes)}
${historySection(done, full)}
</main>
</body>
</html>
`;
}
