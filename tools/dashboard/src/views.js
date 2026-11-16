// The page: pure functions from data to HTML. Each view is a section with
// data-region so the browser can refresh it in place. Without JavaScript every
// view shows, in order; the script shows one at a time, chosen by the hash.
//
// Owner controls exist only inside [data-controls] wrappers rendered here. A
// wrapper carries what the page showed (the newest reply id, or the docs
// commit), so the server can refuse an action on something the owner has not
// read. Markdown never adds elements of its own: raw HTML is escaped.
import { h, raw } from './html.js';
import { renderInline, renderMarkdown } from './markdown.js';
import { DOC_FILES, anchorText, lineDiff, noteTargets, parseDecisions, parseRestart, productLayout, slugify, splitSections, uniqueId } from './docs.js';
import { noteSummary } from './private.js';
import { forYouOrder, lastActivity, lastReplyId } from './state.js';
import { VERDICT_CHIP, prepareStack, stackView } from './stack-view.js';

export const KIND_PILL = { decide: 'Needs your call', look: 'An agent chose', shipped: 'Shipped', note: 'An agent replied to your note' };
const KIND_SHORT = { decide: 'Decide', look: 'Look', shipped: 'Shipped', note: 'Note' };
const STATE_LABEL = { waiting: 'waiting for you', answered: 'answered', closed: 'closed' };
// Reactions on journey steps, which older stores still hold.
const REACTION_WORD = { essential: 'essential', nice: 'nice to have', unneeded: 'not needed' };
const FILE_LABEL = { [DOC_FILES.product]: 'Product', [DOC_FILES.decisions]: 'Decisions', [DOC_FILES.restart]: 'Restart' };
const CHANGED_WORD = { changed: 'Changed', new: 'New', removed: 'Removed' };
const ROW_KIND = { decision: 'Decision', verdict: 'Verdict', answer: 'Answer' };

const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
export const minutesFor = (count) => Math.max(1, Math.round(0.6 * count));

function time(iso) {
  if (!iso) return '';
  const date = new Date(iso);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  const text = date.toLocaleString('en-GB', { day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }), hour: '2-digit', minute: '2-digit' });
  return h`<time datetime="${iso}" title="${iso}">${text}</time>`;
}

// The local calendar day of a timestamp, as YYYY-MM-DD.
export function localDay(iso) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

// ---- anchors of owner notes ----

const sectionAnchor = (key) => `product#${key}`;
const orphanId = (anchor) => `orphan-${slugify(anchor)}`;

// Where a note's anchor is on the page: its product section while PRODUCT.md
// still has it, else its entry under "Notes on things that changed". Anchors
// of other kinds (journey notes from an earlier version) have no place.
function anchorHref(anchor, ctx) {
  const match = /^product#(.+)$/.exec(anchor ?? '');
  if (!match) return null;
  return ctx.targets.has(anchor) ? `#product-${match[1]}` : `#${orphanId(anchor)}`;
}

// anchor -> { replies (every note item's replies, oldest first), notes (owner
// texts), after, anchorText (as last recorded), open (the open note item) }
function noteIndex(items) {
  const index = new Map();
  for (const item of items) {
    if (item.kind !== 'note' || !item.anchor) continue;
    if (!index.has(item.anchor)) index.set(item.anchor, { replies: [], after: 0, open: null });
    const entry = index.get(item.anchor);
    entry.replies.push(...item.replies);
    entry.anchorText = item.anchor_text || item.title;
    if (item.status === 'open') {
      entry.after = Math.max(entry.after, lastReplyId(item));
      entry.open = item;
    }
  }
  for (const entry of index.values()) {
    entry.replies.sort((a, b) => a.id - b.id);
    entry.notes = entry.replies.filter((reply) => reply.author === 'owner' && reply.text).length;
  }
  return index;
}

const EMPTY_NOTES = { replies: [], notes: 0, after: 0 };

// ---- replies and attachments ----

function replyLine(reply) {
  const mine = reply.author === 'owner';
  const marks = [];
  if (reply.verdict) marks.push(reply.verdict === 'agree' ? 'agreed' : 'disagreed');
  if (reply.choice != null) marks.push(`picked option ${reply.choice + 1}`);
  if (reply.reaction) marks.push(REACTION_WORD[reply.reaction]);
  if (mine && !reply.text && !reply.reaction && !reply.verdict) marks.push('cleared the reaction');
  let text = '';
  if (reply.choice != null) text = h`<div class="prose"><p>${reply.text}</p></div>`;
  else if (reply.text) text = h`<div class="prose">${raw(renderMarkdown(reply.text))}</div>`;
  return h`<li class="reply ${mine ? 'from-owner' : 'from-agent'}">
    <div class="reply-meta"><b>${mine ? 'You' : 'Agent'}</b>${marks.length ? ` · ${marks.join(' · ')}` : ''} · ${time(reply.created_at)}${mine && reply.acked_at ? ' · read by an agent' : ''}</div>
    ${text}
  </li>`;
}

const thread = (replies) => (replies.length ? h`<ol class="thread">${replies.map(replyLine)}</ol>` : '');

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

// ---- owner controls ----

function sayForm({ api = null, verdict = null, note = false, draft, placeholder, open = false, sendButton = true }) {
  return h`<form class="say ${open ? 'on' : ''}" ${api ? h`data-api="${api}"` : ''} ${verdict ? h`data-verdict="${verdict}"` : ''} ${note ? raw('data-note') : ''} data-draft="${draft}">
    <textarea name="text" rows="3" placeholder="${placeholder}" aria-label="${placeholder}"></textarea>
    <div class="row"><span>Agents read this at the start of their next session.</span>${sendButton ? h`<button class="btn go" type="submit">Send <kbd>⌘↵</kbd></button>` : ''}</div>
  </form>`;
}

function optionsView(item) {
  return h`<div class="options" role="group" aria-label="Options">${item.options.map(
    (option, index) => h`<button class="option" type="button" data-post="/api/items/${item.id}/replies" data-body="${JSON.stringify({ choice: index })}"><span class="radio" aria-hidden="true"></span><strong>${option.label}</strong>${option.recommended ? h`<span class="rec">recommended</span>` : h`<span></span>`}${option.detail ? h`<small>${option.detail}</small>` : ''}</button>`,
  )}</div>`;
}

// The controls of a card in the For you queue. The button marked data-primary
// is what Enter does.
function queueControls(item) {
  const api = `/api/items/${item.id}/replies`;
  const later = h`<button class="btn ghost skip" type="button" data-skip>Later <kbd>→</kbd></button>`;
  const say = (label) => h`<button class="btn" type="button" data-say aria-expanded="false">${label}</button>`;
  let options = '';
  let acts;
  let form;
  if (item.kind === 'look') {
    acts = h`<button class="btn go" type="button" data-primary data-post="${api}" data-body="${JSON.stringify({ verdict: 'agree' })}">Fine, go ahead <kbd>↵</kbd></button>${say('No, do this instead…')}${later}`;
    form = sayForm({ api, verdict: 'disagree', draft: `item-${item.id}-disagree`, placeholder: 'What should happen instead?' });
  } else if (item.kind === 'decide' && item.options) {
    options = optionsView(item);
    const recommended = item.options.findIndex((option) => option.recommended);
    const primary =
      recommended === -1
        ? ''
        : h`<button class="btn go" type="button" data-primary data-post="${api}" data-body="${JSON.stringify({ choice: recommended })}">Pick the recommended <kbd>↵</kbd></button>`;
    acts = h`${primary}${say('Say something…')}${later}`;
    form = sayForm({ api, draft: `item-${item.id}-reply`, placeholder: 'Your answer, in any form' });
  } else if (item.kind === 'decide') {
    acts = h`<button class="btn go" type="button" data-primary data-submit>Send <kbd>↵</kbd></button>${later}`;
    form = sayForm({ api, draft: `item-${item.id}-reply`, placeholder: 'Your answer, in any form', open: true, sendButton: false });
  } else {
    acts = h`<button class="btn go" type="button" data-primary data-post="/api/items/${item.id}/seen" data-body="{}">Got it <kbd>↵</kbd></button>${say('Say something…')}${later}`;
    form = sayForm({ api, draft: `item-${item.id}-reply`, placeholder: 'Your thoughts, in any form' });
  }
  return h`<div class="controls" data-controls data-item="${item.id}" data-after="${lastReplyId(item)}">
    ${options}<div class="acts">${acts}</div>${form}<p class="form-error" role="alert"></p>
  </div>`;
}

// The only control on an answered item: say something more.
function commentControls(item) {
  return h`<div class="controls" data-controls data-item="${item.id}" data-after="${lastReplyId(item)}">
    <div class="acts"><button class="btn small" type="button" data-say aria-expanded="false">Say something…</button></div>
    ${sayForm({ api: `/api/items/${item.id}/replies`, draft: `item-${item.id}-comment`, placeholder: 'Your thoughts, in any form' })}
    <p class="form-error" role="alert"></p>
  </div>`;
}

// "＋ note" for one product section.
function noteControls({ anchor, anchorText, info = EMPTY_NOTES }) {
  return h`<div class="note-controls" data-controls data-anchor="${anchor}" data-anchor-text="${anchorText}" data-after="${info.after}">
    <div class="react"><button class="pill" type="button" data-say aria-expanded="false">＋ note</button></div>
    ${sayForm({ note: true, draft: `note-${anchor}`, placeholder: 'Your note, in any form' })}
    <p class="form-error" role="alert"></p>
  </div>`;
}

// ---- For you ----

// What a card is about: a note's product section, or the list entry the item
// names (a link to it in the Stack while it is on the list).
function whereLink(item, ctx) {
  if (item.kind === 'note') {
    const href = anchorHref(item.anchor, ctx);
    const label = item.anchor_text || item.title;
    return href ? h`<a href="${href}"><b>${label}</b></a>` : h`<b>${label}</b>`;
  }
  if (!item.journey) return '';
  const entry = ctx.entryById.get(item.journey);
  return entry ? h`<a href="#feature-${entry.id}"><b>${entry.name}</b></a>` : h`<b>${item.journey}</b>`;
}

function cardContent(item, ctx) {
  const where = whereLink(item, ctx);
  return h`<div class="card-top"><span class="kind ${item.kind}">${KIND_PILL[item.kind]}</span>${where ? h`<span class="where">on ${where}</span>` : ''}</div>
    <h2>${item.title}</h2>
    ${item.body ? h`<div class="body prose">${raw(renderMarkdown(item.body))}</div>` : ''}
    ${item.default_text ? h`<div class="stands"><span>If you say nothing</span><div class="prose">${raw(renderMarkdown(item.default_text))}</div></div>` : ''}
    ${item.attachments.length ? h`<div class="attachments">${item.attachments.map(attachmentView)}</div>` : ''}
    ${thread(item.replies)}`;
}

function queueCard(item, ctx) {
  return h`<article class="card" id="item-${item.id}" data-card="${item.id}" tabindex="-1" aria-label="${KIND_PILL[item.kind]}: ${item.title}">
    ${cardContent(item, ctx)}
    ${queueControls(item)}
  </article>`;
}

function answeredRow(item, ctx) {
  return h`<details class="done-row" id="item-${item.id}">
    <summary><span class="kind ${item.kind} small">${KIND_SHORT[item.kind]}</span><span class="row-title">${item.title}</span><span class="done-meta">${STATE_LABEL[item.state]} · ${time(lastActivity(item))}</span></summary>
    <div class="done-body">
      ${cardContent(item, ctx)}
      ${item.status === 'closed' ? h`<p class="closed-note">Closed ${time(item.closed_at)}${item.close_note ? h`: ${item.close_note}` : ''}</p>` : commentControls(item)}
    </div>
  </details>`;
}

function forYouView(ctx) {
  const { waiting, done } = ctx;
  const count = waiting.length;
  const lede = count ? h`${plural(count, 'thing')} <em>· about ${plural(minutesFor(count), 'minute')}</em>` : 'All clear';
  const empty = h`<div class="card empty"><div class="tick" aria-hidden="true">✓</div><h2>Nothing is waiting for you</h2>${ctx.stack ? h`<a class="btn go" href="#stack">Go to the Stack</a>` : ''}</div>`;
  return h`<section class="view" id="for-you" data-region="for-you" aria-label="For you">
  <div class="queue" data-queue>
    <div class="queue-head">
      <div><p class="eyebrow">For you</p><p class="lede">${lede}</p></div>
      <div class="queue-tools">${count > 1 ? h`<button class="btn ghost small" type="button" data-see-all aria-pressed="false">See all</button>` : ''}<div class="dots" data-dots></div></div>
    </div>
    <div class="stage" aria-live="polite">${count ? waiting.map((item) => queueCard(item, ctx)) : empty}</div>
    ${count > 1 ? h`<ol class="rows">${waiting.map((item) => h`<li><button class="row" type="button" data-focus="${item.id}"><span class="kind ${item.kind} small">${KIND_SHORT[item.kind]}</span><span class="row-title">${item.title}</span></button></li>`)}</ol>` : ''}
    ${count ? h`<div class="hints" aria-hidden="true"><span><kbd>↵</kbd> accept</span><span><kbd>→</kbd> later</span><span><kbd>←</kbd> back</span><span><kbd>⇥</kbd> say something else</span></div>` : ''}
    ${done.length ? h`<details class="earlier" id="earlier-items"><summary>Earlier items (${done.length})</summary><div class="answered-list">${done.map((item) => answeredRow(item, ctx))}</div></details>` : ''}
  </div>
</section>`;
}

// ---- Product ----

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
    if (skipped) out.push(h`<div class="diff-gap">${plural(skipped, 'unchanged line')}</div>`);
    skipped = 0;
    out.push(h`<div class="diff-line diff-${row.op === '+' ? 'add' : row.op === '-' ? 'del' : 'ctx'}"><span class="diff-op">${row.op === ' ' ? '' : row.op}</span><span>${row.line || ' '}</span></div>`);
  });
  if (skipped) out.push(h`<div class="diff-gap">${plural(skipped, 'unchanged line')}</div>`);
  return h`<div class="diff" role="group" aria-label="Before and after">${out}</div>`;
}

const changeKey = (change) => `${slugify(change.file)}-${change.key || 'intro'}`;

const markSeen = (head, label) =>
  h`<div class="mark-seen" data-controls><button class="btn small" type="button" data-post="/api/docs/seen" data-body="${JSON.stringify({ commit: head })}">${label}</button><p class="form-error" role="alert"></p></div>`;

// "Changed since you last looked", over PRODUCT.md, DECISIONS.md and
// RESTART.md. Before the owner first marks them read (or when that commit is
// gone) there is nothing to compare with: one line and the button.
function changedStrip(ctx) {
  const { changes, state, head } = ctx.changes;
  if (state === 'nogit' || !head) return '';
  if (state === 'never' || state === 'unknown') {
    return h`<div class="changed-strip" id="changed"><p>Mark as read to see what changes from now on</p>${markSeen(head, 'Mark as read')}</div>`;
  }
  if (!changes.length) return '';
  return h`<div class="changed-strip" id="changed">
    <b>Changed since you last looked</b>
    <ul>${changes.map((change) => h`<li><details class="change" id="change-${changeKey(change)}">
      <summary><span>${FILE_LABEL[change.file] || change.file} · ${change.heading || 'Introduction'}</span><span class="tag ${change.status}">${CHANGED_WORD[change.status]}</span></summary>
      ${diffView(change)}
    </details></li>`)}</ul>
    ${markSeen(head, 'Mark all as seen')}
  </div>`;
}

// Notes whose section PRODUCT.md no longer has: kept, with their thread and
// a box to reply on the open note item.
function orphanNotes(ctx) {
  const gone = [...ctx.notes.entries()].filter(([anchor]) => anchor.startsWith('product#') && !ctx.targets.has(anchor));
  if (!gone.length) return '';
  return h`<div class="orphans"><p class="eyebrow">Notes on things that changed</p>${gone.map(
    ([anchor, info]) => h`<div class="orphan" id="${orphanId(anchor)}">
      <div class="orphan-head"><b>${info.anchorText}</b></div>
      ${thread(info.replies)}
      ${info.open ? commentControls(info.open) : ''}
    </div>`,
  )}</div>`;
}

function changeMark(change) {
  if (!change) return '';
  return h`<details class="change-mark" id="mark-${changeKey(change)}"><summary><span class="mark ${change.status}">${CHANGED_WORD[change.status]}</span></summary>${diffView(change)}</details>`;
}

// The loop: part of the page, not of the document.
const LOOP = raw(`<svg viewBox="0 0 820 300" role="img" aria-label="The loop: main is copied into task computers, you see what changed, then merge or drop">
  <text class="stage-label" x="110" y="26" text-anchor="middle">1 · a computer</text>
  <text class="stage-label" x="400" y="26" text-anchor="middle">2 · a copy per task</text>
  <text class="stage-label" x="690" y="26" text-anchor="middle">3 · what changed</text>
  <path class="wire" d="M190 150 C 250 150, 260 88, 320 88"/>
  <path class="wire" d="M190 150 C 250 150, 260 212, 320 212"/>
  <path class="wire" d="M480 88 C 540 88, 560 150, 610 150"/>
  <path class="wire" d="M480 212 C 540 212, 560 150, 610 150"/>
  <g>
    <rect class="box cycle pulse" x="30" y="100" width="160" height="100" rx="16"/>
    <rect class="screen" x="46" y="116" width="128" height="50" rx="7"/>
    <rect class="fill-built" x="56" y="127" width="62" height="5" rx="2.5"/>
    <rect class="fill-line" x="56" y="138" width="92" height="5" rx="2.5"/>
    <rect class="fill-line" x="56" y="149" width="44" height="5" rx="2.5"/>
    <text class="lbl" x="110" y="187" text-anchor="middle">main</text>
    <text x="110" y="226" text-anchor="middle">your setup, your agent</text>
  </g>
  <g class="cycle fork-a">
    <rect class="box" x="320" y="48" width="160" height="80" rx="14"/>
    <rect class="screen" x="334" y="62" width="132" height="36" rx="6"/>
    <rect class="cycle work fill-accent" x="343" y="71" width="100" height="5" rx="2.5"/>
    <rect class="cycle work fill-accent half" x="343" y="82" width="64" height="5" rx="2.5"/>
    <text class="lbl" x="400" y="117" text-anchor="middle">fix-login</text>
  </g>
  <g class="cycle fork-b">
    <rect class="box" x="320" y="172" width="160" height="80" rx="14"/>
    <rect class="screen" x="334" y="186" width="132" height="36" rx="6"/>
    <rect class="cycle work fill-untested" x="343" y="195" width="84" height="5" rx="2.5"/>
    <rect class="cycle work fill-untested half" x="343" y="206" width="110" height="5" rx="2.5"/>
    <text class="lbl" x="400" y="241" text-anchor="middle">try-redis</text>
  </g>
  <g class="cycle diffcard">
    <rect class="box" x="610" y="92" width="170" height="116" rx="14"/>
    <text class="small" x="626" y="118"><tspan class="plus">+ </tspan>code · 3 commits</text>
    <text class="small" x="626" y="140"><tspan class="plus">+ </tspan>packages · redis</text>
    <text class="small" x="626" y="162"><tspan class="tilde">~ </tspan>settings · 2 files</text>
    <text class="small" x="626" y="184"><tspan class="tilde">~ </tspan>database · 1 table</text>
  </g>
  <g class="cycle verdict-m"><rect class="merge-pill" x="640" y="222" width="110" height="28" rx="14"/><text class="merge-text" x="695" y="241" text-anchor="middle">merge ✓</text></g>
  <g class="cycle verdict-d"><rect class="drop-pill" x="640" y="50" width="110" height="28" rx="14"/><text class="drop-text" x="695" y="69" text-anchor="middle">drop ×</text></g>
  <text x="400" y="288" text-anchor="middle">Two tasks never share a port, a database or a checkout.</text>
</svg>`);

function productView(ctx) {
  const text = ctx.docs.product;
  if (text === null) {
    return h`<section class="view" id="product" data-region="product" aria-label="Product">${changedStrip(ctx)}<p class="eyebrow">The product</p><p class="empty-note">No product description yet (docs/product/PRODUCT.md is missing).</p></section>`;
  }
  const layout = productLayout(text);
  const change = (key) => ctx.changeIndex.get(`${DOC_FILES.product}#${key}`);
  return h`<section class="view" id="product" data-region="product" aria-label="Product">
  ${changedStrip(ctx)}
  <div class="hero" ${layout.hero.key ? h`id="product-${layout.hero.key}"` : ''}>
    <p class="eyebrow">The product</p>
    <div class="hero-text">${raw(renderMarkdown(layout.hero.text))}</div>
    ${layout.hero.key ? changeMark(change(layout.hero.key)) : ''}
  </div>
  <div class="loop">${LOOP}</div>
  <div class="grid">${layout.tiles.map((tile) => {
    const anchor = sectionAnchor(tile.key);
    const info = ctx.notes.get(anchor) || EMPTY_NOTES;
    return h`<article class="tile ${tile.wide ? 'wide' : ''}" id="product-${tile.key}">
      <div class="tile-head"><h3>${tile.heading}</h3>${changeMark(change(tile.key))}</div>
      <div class="prose">${raw(renderMarkdown(tile.text))}</div>
      ${info.replies.length ? h`<details class="notes-of" id="notes-product-${tile.key}"><summary>${plural(info.notes, 'note')} from you</summary>${thread(info.replies)}</details>` : ''}
      ${noteControls({ anchor, anchorText: anchorText(tile.heading), info })}
    </article>`;
  })}</div>
  ${orphanNotes(ctx)}
</section>`;
}

// ---- Decisions: one timeline of what was decided ----

const VERDICT_ORDER = ['take', 'refine', 'later', 'drop'];
const VERDICT_PHRASE = {
  take: (n) => `took ${plural(n, 'entry', 'entries')}`,
  refine: (n) => `asked to refine ${n}`,
  later: (n) => `kept ${n} for later`,
  drop: (n) => `dropped ${n}`,
};

// "You took 6 entries, kept 1 for later, dropped 2".
export function verdictSummary(entries) {
  const counts = new Map();
  for (const entry of entries) counts.set(entry.verdict, (counts.get(entry.verdict) ?? 0) + 1);
  const parts = VERDICT_ORDER.filter((verdict) => counts.get(verdict)).map((verdict) => VERDICT_PHRASE[verdict](counts.get(verdict)));
  return parts.length ? `You ${parts.join(', ')}` : '';
}

// What the owner answered in one reply, in words.
function answerWords(item, reply) {
  if (reply.verdict === 'agree') return reply.text ? `Agreed: ${reply.text}` : 'Agreed';
  if (reply.verdict === 'disagree') return `Disagreed: ${reply.text}`;
  if (reply.choice != null) return `Chose “${reply.choice_label ?? reply.text}”`;
  if (reply.reaction) return `Marked ${REACTION_WORD[reply.reaction] ?? reply.reaction}`;
  if (!reply.text) return item.kind === 'note' ? 'Cleared the reaction' : '';
  return item.kind === 'note' ? `Note: ${reply.text}` : reply.text;
}

const oneLine = (text, max = 240) => {
  const line = String(text).replace(/\s+/g, ' ').trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
};

/**
 * Everything decided, as one timeline: { days: [{ date, decisions, verdicts,
 * answers }] newest first, other (DECISIONS.md items that are not
 * `date · who · text`), carried (verdicts that come only from
 * features.jsonl), total (DECISIONS.md items) }.
 * - decisions: the lines of DECISIONS.md for that day, in file order.
 * - verdicts: the current verdict of each entry whose newest store row is
 *   from that day: [{ id, name, verdict, note, onList }].
 * - answers: the owner's replies on items from that day, newest first:
 *   [{ item, reply }].
 * `verdictRows` is the store's newest verdict row per entry; `features` the
 * current list (or null).
 */
export function decisionTimeline({ decisionsText, features, verdictRows, items }) {
  const decisions = parseDecisions(decisionsText);
  const days = new Map();
  const day = (date) => {
    if (!days.has(date)) days.set(date, { date, decisions: [], verdicts: [], answers: [] });
    return days.get(date);
  };
  for (const { date, items: lines } of decisions.days) day(date).decisions.push(...lines);
  const byId = new Map((features ?? []).map((feature) => [feature.id, feature]));
  const hasRow = new Set();
  for (const row of verdictRows ?? []) {
    hasRow.add(row.feature_id);
    const date = localDay(row.created_at);
    if (!row.verdict || !date) continue;
    const feature = byId.get(row.feature_id);
    day(date).verdicts.push({ id: row.feature_id, name: feature?.name ?? row.name ?? row.feature_id, verdict: row.verdict, note: row.note || '', onList: Boolean(feature), n: feature?.n ?? Infinity });
  }
  const carried = (features ?? [])
    .filter((feature) => feature.verdict && !hasRow.has(feature.id))
    .map((feature) => ({ id: feature.id, name: feature.name, verdict: feature.verdict, note: '', onList: true, n: feature.n }));
  for (const item of items) {
    for (const reply of item.replies) {
      const date = reply.author === 'owner' && localDay(reply.created_at);
      if (date) day(date).answers.push({ item, reply });
    }
  }
  for (const entry of days.values()) {
    entry.verdicts.sort((a, b) => a.n - b.n);
    entry.answers.sort((a, b) => b.reply.id - a.reply.id);
  }
  const sorted = [...days.values()].sort((a, b) => (a.date < b.date ? 1 : a.date > b.date ? -1 : 0));
  return { days: sorted, other: decisions.other, carried, total: decisions.total };
}

function dayLabel(date) {
  const [year, month, day] = date.split('-').map(Number);
  const value = new Date(year, month - 1, day);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = Math.round((today - value) / 86_400_000);
  const label = value.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', ...(year === today.getFullYear() ? {} : { year: 'numeric' }) });
  return { label, relative: days === 0 ? 'today' : days === 1 ? 'yesterday' : '' };
}

const kindLabel = (kind) => h`<span class="rkind ${kind}">${ROW_KIND[kind]}</span>`;
const who = (name) => h`<span class="who ${name === 'owner' ? 'you' : ''}">${name === 'owner' ? 'you' : name}</span>`;

function verdictFold(entries, id) {
  return h`<details class="dec fold" data-kind="verdict" id="${id}">
    <summary>${kindLabel('verdict')}${who('owner')}<span class="what">${verdictSummary(entries)}</span></summary>
    <ul class="fold-list">${entries.map(
      (entry) => h`<li>${entry.onList ? h`<a href="#feature-${entry.id}">${entry.name}</a>` : h`<span>${entry.name}</span>`}<span class="vchip small v-${entry.verdict}">${VERDICT_CHIP[entry.verdict]}</span>${entry.note ? h`<span class="fold-note">${oneLine(entry.note)}</span>` : ''}</li>`,
    )}</ul>
  </details>`;
}

// Where an answer's item is on the page: its card in For you (or its row
// under "Earlier items"), or a note's product section.
function itemHref(item, ctx) {
  if (item.state === 'waiting' || item.kind !== 'note') return `#item-${item.id}`;
  return anchorHref(item.anchor, ctx);
}

function answerRow({ item, reply }, ctx) {
  const title = item.kind === 'note' ? item.anchor_text || item.title : item.title;
  const href = itemHref(item, ctx);
  const words = answerWords(item, reply);
  return h`<div class="dec" data-kind="answer">${kindLabel('answer')}${who('owner')}<span class="what">${href ? h`<a href="${href}">${title}</a>` : h`<b>${title}</b>`}${words ? h` · ${oneLine(words)}` : ''}</span></div>`;
}

function decisionsView(ctx) {
  const { days, other, carried, total } = ctx.timeline;
  const empty = !days.length && !other.length && !carried.length;
  const filters = [
    ['all', 'All'],
    ['decision', 'Decisions'],
    ['verdict', 'Verdicts'],
    ['answer', 'Answers'],
  ];
  return h`<section class="view" id="decisions" data-region="decisions" aria-label="Decisions">
  <p class="eyebrow">Decisions</p>
  <p class="lede">Everything decided <em>· agents read this before asking</em></p>
  <div class="filter-row">
    <input class="search" type="search" id="decision-filter" placeholder="Search everything decided" aria-label="Search everything decided" autocomplete="off">
    <span class="count" id="decision-count" data-total="${total}">${plural(total, 'decision')}</span>
  </div>
  <div class="kind-filters" role="group" aria-label="Show">${filters.map(
    ([value, label]) => h`<button class="pill ${value === 'all' ? 'on' : ''}" type="button" data-decision-kind="${value}" aria-pressed="${value === 'all' ? 'true' : 'false'}">${label}</button>`,
  )}</div>
  ${empty ? h`<p class="empty-note">Nothing decided yet.</p>` : ''}
  <div id="decision-list">
    ${days.map((day) => {
      const { label, relative } = dayLabel(day.date);
      return h`<div class="day"><time datetime="${day.date}">${label}${relative ? h`<small>${relative}</small>` : ''}</time><div>
        ${day.decisions.map((item) => h`<div class="dec" data-kind="decision">${kindLabel('decision')}${who(item.who)}<span class="what">${raw(renderInline(item.text))}</span></div>`)}
        ${day.verdicts.length ? verdictFold(day.verdicts, `verdicts-${day.date}`) : ''}
        ${day.answers.map((answer) => answerRow(answer, ctx))}
      </div></div>`;
    })}
    ${other.length ? h`<div class="day"><time>Other</time><div>${other.map((entry) => h`<div class="dec other" data-kind="decision">${kindLabel('decision')}<span></span><div class="what prose">${raw(renderMarkdown(entry))}</div></div>`)}</div></div>` : ''}
    ${carried.length ? h`<div class="day carried"><time>Carried over from earlier decisions</time><div>${verdictFold(carried, 'verdicts-carried')}</div></div>` : ''}
  </div>
</section>`;
}

// ---- Private ----

// Unique note ids, in listing order: two paths that slugify alike get -2, -3.
function withNoteIds(notes) {
  const used = new Set();
  return notes.map((note) => ({ ...note, id: `note-${uniqueId(slugify(note.path), used, 'note')}` }));
}

function readerView(note) {
  const { id } = note;
  const { preamble, sections } = splitSections(note.text);
  return h`<article class="reader" id="${id}">
    <a class="back" href="#private">← All notes</a>
    <div class="reader-in">
      <div class="prose reader-text">
        ${preamble.trim() ? raw(renderMarkdown(preamble)) : ''}
        ${sections.map((section) => h`<section id="${id}--${section.key}">${raw(renderMarkdown(section.text))}</section>`)}
      </div>
      ${sections.length ? h`<nav class="toc" aria-label="In this note">${sections.map((section) => h`<a href="#${id}--${section.key}">${section.heading}</a>`)}</nav>` : ''}
    </div>
  </article>`;
}

function privateView(groups) {
  const notes = withNoteIds(groups.flatMap((group) => group.notes.map((note) => ({ ...note, folder: group.folder }))));
  if (!notes.length) return '';
  return h`<section class="view" id="private" data-region="private" aria-label="Private">
  <div class="notes-index">
    <p class="eyebrow">Private</p>
    <p class="lede">Kept on this Mac <em>· not in the repository</em></p>
    <div class="grid">${notes.map((note) => {
      const { title, summary } = noteSummary(note);
      return h`<a class="tile note-tile" href="#${note.id}"><h3>${note.folder || 'private'}</h3><p class="big">${title}</p>${summary ? h`<p>${summary}</p>` : ''}</a>`;
    })}</div>
  </div>
  ${notes.map(readerView)}
</section>`;
}

// ---- page ----

function topBar(ctx) {
  const head = ctx.changes.head;
  const tab = (view, label, badge = null) => h`<a class="tab" href="#${view}" data-view="${view}">${label}${badge === null ? '' : h` <span class="badge quiet">${badge}</span>`}</a>`;
  const waiting = ctx.waiting.length;
  return h`<header class="bar" data-region="nav"><div class="bar-in">
  <a class="brand" href="#${ctx.defaultView}"><i aria-hidden="true"></i>Silo</a>
  <nav class="tabs" aria-label="Views">${ctx.stack ? tab('stack', 'Stack', ctx.stack.totals.all.undecided) : ''}<a class="tab" href="#for-you" data-view="for-you">For you${waiting ? h` <span class="badge">${waiting}</span>` : ''}</a>${tab('decisions', 'Decisions', ctx.docs.decisions === null ? null : ctx.timeline.total)}${tab('product', 'Product')}${ctx.noteCount ? tab('private', 'Private') : ''}</nav>
  <span class="spacer"></span>
  ${head ? h`<span class="fresh" title="The product documents as of ${head}">docs at ${head.slice(0, 7)}</span>` : h`<span class="fresh off">docs outside git</span>`}
</div></header>`;
}

/**
 * view = { items (with state), docs: {product, decisions, restart} (text or null), changes: docChanges result,
 *   notes: readPrivateNotes result (optional), stack: { features, stages, skipped, rows } or null (optional),
 *   verdictRows: the store's newest verdict row per entry (optional; defaults to stack.rows) }
 */
export function renderPage(view) {
  const items = view.items;
  const waiting = items.filter((item) => item.state === 'waiting').sort(forYouOrder);
  const done = items.filter((item) => item.state !== 'waiting' && item.kind !== 'note').sort((a, b) => (lastActivity(a) < lastActivity(b) ? 1 : -1));
  const groups = view.notes ?? [];
  const stack = prepareStack(view.stack ?? null);
  const ctx = {
    ...view,
    waiting,
    done,
    notes: noteIndex(items),
    targets: noteTargets(view.docs),
    changeIndex: new Map(view.changes.changes.map((change) => [`${change.file}#${change.key}`, change])),
    timeline: decisionTimeline({ decisionsText: view.docs.decisions, features: stack?.features ?? null, verdictRows: view.verdictRows ?? view.stack?.rows ?? [], items }),
    noteCount: groups.reduce((sum, group) => sum + group.notes.length, 0),
    stack,
    entryById: new Map((stack?.features ?? []).map((feature) => [feature.id, feature])),
    defaultView: stack ? 'stack' : 'for-you',
  };
  return h`<!doctype html>
<html lang="en" data-default="${ctx.defaultView}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light dark">
<title>Silo owner</title>
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='9' fill='%231b1a17'/%3E%3Ccircle cx='16' cy='16' r='6' fill='%23f6f5f2'/%3E%3C/svg%3E">
<link rel="stylesheet" href="/app.css">
<script src="/boot.js"></script>
<script type="module" src="/app.js"></script>
</head>
<body>
${topBar(ctx)}
<main>
<div class="lost" id="lost" aria-live="polite"></div>
${stackView(stack, parseRestart(view.docs.restart))}
${forYouView(ctx)}
${decisionsView(ctx)}
${productView(ctx)}
${privateView(groups)}
</main>
</body>
</html>
`;
}
