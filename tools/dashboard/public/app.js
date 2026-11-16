// The page's behaviour. Everything is server-rendered HTML; this script picks
// the view from the hash, shows the For you queue one card at a time, and
// sends owner actions. After an action the page fetches itself again and
// replaces its regions, so what shows always comes from the server.
//
// Owner actions start only from controls inside a server-rendered
// [data-controls] wrapper. The wrapper's data-after (the newest reply the page
// showed) goes with the action; when the item changed since, the server
// answers 409 and the page shows it again with a note.

const root = document.documentElement;
const viewFor = window.siloViewFor || (() => 'for-you');
const CHANGED_NOTE = 'This changed while you were reading. Read it again, then answer.';
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, reducedMotion.matches ? 0 : ms));

const state = { at: null, done: 0, all: false, gaps: false, busy: false, shown: null, readyAt: 0 };
// A card that just became current takes no primary action for this long, so a
// held or repeated Enter cannot answer a card the owner has not read.
const READY_DELAY = 700;

// ---- views ----

function hashId() {
  try {
    return decodeURIComponent(location.hash.slice(1));
  } catch {
    return '';
  }
}

// Shows the view the address names. `queue: false` keeps the queue on its
// current card (after an action, the card acted on stays in charge). Whatever
// fails, one view stays visible.
function route(options = {}) {
  try {
    showRoute(options);
  } catch (error) {
    root.dataset.view = document.getElementById(root.dataset.view || '') ? root.dataset.view : 'for-you';
    console.error('route failed', error);
  }
}

function showRoute({ scroll = true, queue = true } = {}) {
  let view;
  try {
    view = viewFor(location.hash);
  } catch {
    view = 'for-you';
  }
  if (!document.getElementById(view)) view = 'for-you';
  root.dataset.view = view;
  document.querySelectorAll('.tab[data-view]').forEach((tab) => {
    if (tab.dataset.view === view) tab.setAttribute('aria-current', 'page');
    else tab.removeAttribute('aria-current');
  });
  const id = hashId();
  const target = id ? document.getElementById(id) : null;
  const reader = view === 'private' && target ? target.closest('.reader') : null;
  document.querySelectorAll('.reader').forEach((article) => article.classList.toggle('on', article === reader));
  document.getElementById('private')?.classList.toggle('reading', Boolean(reader));

  const card = queue && /^item-\d+$/.test(id) ? document.querySelector(`.stage [data-card="${id.slice(5)}"]`) : null;
  if (card) {
    state.at = Number(card.dataset.card);
    state.all = false;
    showQueue({ focus: true });
  }
  if (target && target.tagName === 'DETAILS') target.open = true;
  if (scroll) {
    if (target && !card && target.id !== view && !target.classList.contains('reader')) {
      requestAnimationFrame(() => target.scrollIntoView({ block: 'start' }));
    } else window.scrollTo(0, 0);
  }
  renderMermaid();
}

window.addEventListener('hashchange', () => route());

// ---- the For you queue ----

const cards = () => [...document.querySelectorAll('.stage [data-card]')];

function currentCard() {
  const list = cards();
  return list.find((card) => card.dataset.card === String(state.at)) || list[0] || null;
}

function showQueue({ focus = false, back = false } = {}) {
  const queue = document.querySelector('[data-queue]');
  if (!queue) return;
  const list = cards();
  const card = currentCard();
  state.at = card ? Number(card.dataset.card) : null;
  list.forEach((entry) => {
    entry.classList.toggle('current', entry === card);
    entry.classList.toggle('back', entry === card && back);
    entry.classList.remove('out');
  });
  queue.querySelector('.stage')?.classList.add('picked');
  if (card && Number(card.dataset.card) !== state.shown) {
    state.shown = Number(card.dataset.card);
    holdPrimary(card);
  }
  if (!card) state.shown = null;
  // The address always names the card on screen.
  if (root.dataset.view === 'for-you') {
    const hash = card && !state.all ? `#item-${card.dataset.card}` : '#for-you';
    if (location.hash !== hash) history.replaceState(null, '', hash);
  }
  queue.classList.toggle('all', state.all && list.length > 1);
  queue.querySelector('[data-see-all]')?.setAttribute('aria-pressed', String(state.all));
  const dots = queue.querySelector('[data-dots]');
  if (dots) {
    const parts = [];
    for (let n = 0; n < state.done; n += 1) {
      const dot = document.createElement('span');
      dot.className = 'dot done';
      parts.push(dot);
    }
    if (list.length > 1 || state.done) {
      list.forEach((entry, index) => {
        const dot = document.createElement('button');
        dot.type = 'button';
        dot.className = `dot${entry === card ? ' on' : ''}`;
        dot.dataset.focus = entry.dataset.card;
        dot.setAttribute('aria-label', `Item ${index + 1} of ${list.length}`);
        parts.push(dot);
      });
    }
    dots.replaceChildren(...parts);
  }
  if (focus && card && !state.all) card.focus({ preventScroll: true });
}

function holdPrimary(card) {
  state.readyAt = Date.now() + READY_DELAY;
  const primary = card.querySelector('[data-primary]');
  if (!primary) return;
  primary.classList.add('not-ready');
  primary.setAttribute('aria-disabled', 'true');
  setTimeout(() => {
    primary.classList.remove('not-ready');
    primary.removeAttribute('aria-disabled');
  }, READY_DELAY);
}

const primaryReady = () => Date.now() >= state.readyAt;

async function move(step) {
  const list = cards();
  const card = currentCard();
  if (list.length < 2 || !card) return;
  card.classList.add('out');
  await wait(200);
  state.at = Number(list[(list.indexOf(card) + step + list.length) % list.length].dataset.card);
  showQueue({ focus: true, back: step < 0 });
}

// After an action on a queue card: the card leaves and the next one enters.
async function leaveCard(card) {
  const index = cards().indexOf(card);
  card.classList.add('out');
  await Promise.all([refresh(), wait(200)]);
  state.done += 1;
  const list = cards();
  const next = list[Math.min(Math.max(index, 0), list.length - 1)];
  state.at = next ? Number(next.dataset.card) : null;
  showQueue({ focus: true });
}

// ---- region refresh ----

async function refresh() {
  const open = [...document.querySelectorAll('details[id][open]')].map((element) => element.id);
  const drafts = new Map();
  const openForms = new Set();
  document.querySelectorAll('form[data-draft]').forEach((form) => {
    const text = form.querySelector('textarea')?.value;
    if (text) drafts.set(form.dataset.draft, text);
    if (form.classList.contains('on')) openForms.add(form.dataset.draft);
  });
  const filter = document.getElementById('decision-filter')?.value || '';

  const response = await fetch('/', { headers: { Accept: 'text/html' }, cache: 'no-store' });
  if (!response.ok) throw new Error(`refresh failed (${response.status})`);
  const next = new DOMParser().parseFromString(await response.text(), 'text/html');
  const main = document.querySelector('main');
  let previous = null;
  for (const fresh of next.querySelectorAll('[data-region]')) {
    const current = document.querySelector(`[data-region="${fresh.dataset.region}"]`);
    const node = document.importNode(fresh, true);
    if (current) current.replaceWith(node);
    else if (previous) previous.after(node);
    else main?.append(node);
    if (node.parentElement === main) previous = node;
  }
  document.querySelectorAll('[data-region]').forEach((region) => {
    if (!next.querySelector(`[data-region="${region.dataset.region}"]`)) region.remove();
  });

  for (const id of open) {
    const element = document.getElementById(id);
    if (element && element.tagName === 'DETAILS') element.open = true;
  }
  document.querySelectorAll('form[data-draft]').forEach((form) => {
    const text = drafts.get(form.dataset.draft);
    if (text) form.querySelector('textarea').value = text;
    if (openForms.has(form.dataset.draft)) setSay(form, true);
  });
  const input = document.getElementById('decision-filter');
  if (input) input.value = filter;
  applyDecisionFilter();
  applyGaps();
  document.querySelectorAll('.journey').forEach(markHot);
  const shown = state.shown;
  route({ scroll: false, queue: false });
  showQueue();
  // A refreshed card that stays current keeps whatever wait it had left.
  if (state.shown === shown && shown !== null) {
    const card = currentCard();
    const primary = card?.querySelector('[data-primary]');
    if (primary && !primaryReady()) {
      primary.classList.add('not-ready');
      setTimeout(() => primary.classList.remove('not-ready'), state.readyAt - Date.now());
    }
  }
}

// ---- sending ----

async function send(url, body) {
  const response = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!response.ok) {
    let message = `request failed (${response.status})`;
    try {
      message = (await response.json()).error || message;
    } catch {
      /* keep the generic message */
    }
    const error = new Error(message);
    error.status = response.status;
    throw error;
  }
}

const sameControls = (a, b) =>
  (a.dataset.item !== undefined && a.dataset.item === b.dataset.item) || (a.dataset.anchor !== undefined && a.dataset.anchor === b.dataset.anchor);

// Sends one owner action with what the wrapper showed. Returns false after a
// 409, once the page shows the item again with a note to read it first.
async function act(origin, url, body) {
  const controls = origin.closest('[data-controls]');
  if (controls.dataset.after !== undefined) body.after = Number(controls.dataset.after);
  try {
    await send(url, body);
    return true;
  } catch (error) {
    if (error.status !== 409 || (controls.dataset.item === undefined && controls.dataset.anchor === undefined)) throw error;
    if (controls.dataset.item !== undefined) state.at = Number(controls.dataset.item);
    await refresh();
    document.querySelectorAll('[data-controls]').forEach((other) => {
      if (sameControls(other, controls)) {
        const slot = other.querySelector(':scope > .form-error');
        if (slot) slot.textContent = CHANGED_NOTE;
      }
    });
    return false;
  }
}

function showError(origin, message) {
  const slot = origin.closest('[data-controls]')?.querySelector(':scope > .form-error');
  if (slot) slot.textContent = message;
  else window.alert(message);
}

async function run(origin, action) {
  if (state.busy) return;
  state.busy = true;
  const button = origin.matches('form') ? origin.querySelector('button[type="submit"]') : origin;
  if (button) button.disabled = true;
  const card = origin.closest('.stage [data-card]');
  try {
    if (await action()) {
      if (card) await leaveCard(card);
      else {
        const controls = origin.closest('[data-controls]');
        await refresh();
        if (controls) {
          const again = [...document.querySelectorAll('[data-controls]')].find((other) => sameControls(other, controls));
          again?.querySelector('[data-say]')?.focus({ preventScroll: true });
        }
      }
    }
  } catch (error) {
    showError(origin, error.message);
  } finally {
    if (button?.isConnected) button.disabled = false;
    state.busy = false;
  }
}

const noteBody = (controls, extra) => ({
  anchor: controls.dataset.anchor,
  anchorText: controls.dataset.anchorText,
  ...(controls.dataset.journey ? { journey: controls.dataset.journey } : {}),
  ...extra,
});

// ---- the "say something" boxes ----

function setSay(form, open) {
  form.classList.toggle('on', open);
  form.closest('[data-controls]')?.querySelector('[data-say]')?.setAttribute('aria-expanded', String(open));
}

function openSay(scope) {
  const form = scope.querySelector('form.say');
  if (!form) return;
  setSay(form, true);
  form.querySelector('textarea')?.focus();
}

function closeSay(form) {
  const button = form.closest('[data-controls]')?.querySelector('[data-say]');
  if (!button) {
    form.closest('[data-card]')?.focus({ preventScroll: true });
    return;
  }
  setSay(form, false);
  button.focus();
}

// ---- events ----

document.addEventListener('click', (event) => {
  const button = event.target.closest('button');
  if (!button) return;
  if (button.matches('[data-see-all]')) {
    state.all = !state.all;
    showQueue({ focus: !state.all });
    return;
  }
  if (button.matches('[data-focus]')) {
    state.at = Number(button.dataset.focus);
    state.all = false;
    showQueue({ focus: true });
    return;
  }
  if (button.matches('[data-filter]')) {
    state.gaps = button.dataset.filter === 'gaps';
    applyGaps();
    return;
  }
  if (button.matches('[data-primary]') && !primaryReady()) return;
  if (button.matches('[data-skip]')) {
    move(1);
    return;
  }
  const controls = button.closest('[data-controls]');
  if (!controls) return;
  if (button.matches('[data-say]')) {
    const form = controls.querySelector('form.say');
    if (form?.classList.contains('on')) closeSay(form);
    else openSay(controls);
  } else if (button.matches('[data-submit]')) {
    controls.querySelector('form.say')?.requestSubmit();
  } else if (button.matches('[data-post]')) {
    run(button, () => act(button, button.dataset.post, JSON.parse(button.dataset.body || '{}')));
  } else if (button.matches('[data-reaction]')) {
    const pressed = button.getAttribute('aria-pressed') === 'true';
    run(button, () => act(button, '/api/notes', noteBody(controls, { reaction: pressed ? null : button.dataset.reaction })));
  }
});

document.addEventListener('submit', (event) => {
  const form = event.target.closest('form.say');
  const controls = form?.closest('[data-controls]');
  if (!controls) return;
  event.preventDefault();
  const area = form.querySelector('textarea');
  if (!area.value.trim()) {
    showError(form, 'Write something first.');
    area.focus();
    return;
  }
  const text = area.value;
  const draft = form.dataset.draft;
  const isNote = form.hasAttribute('data-note');
  run(form, async () => {
    let ok = false;
    try {
      if (isNote) ok = await act(form, '/api/notes', noteBody(controls, { text }));
      else ok = await act(form, form.dataset.api, form.dataset.verdict ? { text, verdict: form.dataset.verdict } : { text });
    } finally {
      if (!ok) keepText(form, draft, text, isNote);
    }
    if (ok) {
      area.value = '';
      if (controls.querySelector('[data-say]')) setSay(form, false);
    }
    return ok;
  });
});

// A send that did not go through never loses what the owner typed: it stays
// in its box, or, when the box is gone (the item was closed or answered, or
// the part of the document changed), it shows at the top of For you.
function keepText(form, draft, text, isNote) {
  const box = form.isConnected ? form : [...document.querySelectorAll('form[data-draft]')].find((other) => other.dataset.draft === draft);
  if (box) {
    const area = box.querySelector('textarea');
    if (!area.value.trim()) area.value = text;
    setSay(box, true);
    return;
  }
  const holder = document.getElementById('lost');
  if (!holder) return;
  const notice = document.createElement('div');
  notice.className = 'lost-note';
  notice.setAttribute('role', 'status');
  const message = document.createElement('p');
  message.textContent = isNote
    ? 'The part of the document you wrote about changed meanwhile, so your note was not sent. Your text:'
    : 'This item was closed or answered meanwhile, so your text was not sent. Your text:';
  const copy = document.createElement('textarea');
  copy.readOnly = true;
  copy.rows = 3;
  copy.value = text;
  copy.setAttribute('aria-label', 'Your text that was not sent');
  const row = document.createElement('div');
  row.className = 'row';
  const copyButton = document.createElement('button');
  copyButton.type = 'button';
  copyButton.className = 'btn small';
  copyButton.textContent = 'Copy';
  copyButton.addEventListener('click', async () => {
    copy.select();
    try {
      await navigator.clipboard.writeText(text);
      copyButton.textContent = 'Copied';
    } catch {
      copyButton.textContent = 'Selected: press ⌘C';
    }
  });
  const dismiss = document.createElement('button');
  dismiss.type = 'button';
  dismiss.className = 'btn ghost small';
  dismiss.textContent = 'Dismiss';
  dismiss.addEventListener('click', () => notice.remove());
  row.append(copyButton, dismiss);
  notice.append(message, copy, row);
  holder.append(notice);
  if (root.dataset.view !== 'for-you') location.hash = '#for-you';
  window.scrollTo(0, 0);
}

document.addEventListener('keydown', (event) => {
  if (event.repeat && !event.target.closest?.('textarea, input')) return;
  const field = event.target.closest?.('textarea, input, select, [contenteditable]');
  if (field) {
    const form = field.closest('form.say');
    if (form && event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      form.requestSubmit();
    } else if (form && event.key === 'Escape') {
      event.preventDefault();
      closeSay(form);
    }
    return;
  }
  if (event.metaKey || event.ctrlKey || event.altKey || root.dataset.view !== 'for-you' || state.all) return;
  const card = currentCard();
  if (!card) return;
  const onControl = event.target.closest?.('button, a, summary, video, audio, details');
  const free = event.target === document.body || event.target === root || event.target === card;
  if (event.key === 'Enter' && !onControl) {
    event.preventDefault();
    card.querySelector('[data-primary]')?.click();
  } else if (event.key === 'ArrowRight' && (free || onControl?.matches('button'))) {
    event.preventDefault();
    move(1);
  } else if (event.key === 'ArrowLeft' && (free || onControl?.matches('button'))) {
    event.preventDefault();
    move(-1);
  } else if (event.key === 'Tab' && !event.shiftKey && free) {
    event.preventDefault();
    openSay(card);
  }
});

// ---- journeys ----

function applyGaps() {
  document.getElementById('journeys')?.classList.toggle('gaps-only', state.gaps);
  document.querySelectorAll('[data-filter]').forEach((pill) => {
    const on = (pill.dataset.filter === 'gaps') === state.gaps;
    pill.classList.toggle('on', on);
    pill.setAttribute('aria-pressed', String(on));
  });
}

// The open step's segment of the track grows.
function markHot(journey) {
  const segments = journey.querySelectorAll('.track i');
  journey.querySelectorAll('.step').forEach((step, index) => segments[index]?.classList.toggle('hot', step.open));
}

document.addEventListener(
  'toggle',
  (event) => {
    const element = event.target;
    if (element.matches?.('.step')) markHot(element.closest('.journey'));
    if (element.open) renderMermaid();
  },
  true,
);

// ---- decisions ----

function applyDecisionFilter() {
  const input = document.getElementById('decision-filter');
  const counter = document.getElementById('decision-count');
  if (!input || !counter) return;
  const needle = input.value.trim().toLowerCase();
  let shown = 0;
  document.querySelectorAll('#decision-list .day').forEach((day) => {
    let any = false;
    day.querySelectorAll('.dec').forEach((row) => {
      const match = !needle || row.textContent.toLowerCase().includes(needle);
      row.hidden = !match;
      if (match) {
        shown += 1;
        any = true;
      }
    });
    day.hidden = !any;
  });
  const total = Number(counter.dataset.total);
  counter.textContent = needle ? `${shown} of ${total}` : `${total} decision${total === 1 ? '' : 's'}`;
}

document.addEventListener('input', (event) => {
  if (event.target.id === 'decision-filter') applyDecisionFilter();
});

// ---- diagrams: mermaid loads only when a visible block needs it ----

let mermaidLoading = null;
async function renderMermaid() {
  const blocks = [...document.querySelectorAll('pre.mermaid:not([data-processed])')].filter((block) => block.offsetParent !== null);
  if (!blocks.length) return;
  mermaidLoading ||= import('/vendor/mermaid/mermaid.esm.min.mjs').then((module) => {
    const mermaid = module.default;
    const dark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: dark ? 'dark' : 'neutral', fontFamily: 'inherit' });
    return mermaid;
  });
  try {
    const mermaid = await mermaidLoading;
    await mermaid.run({ nodes: blocks });
  } catch (error) {
    blocks.forEach((block) => block.classList.add('mermaid-failed'));
    console.error('mermaid failed', error);
  }
}

route({ scroll: Boolean(location.hash) });
showQueue();
applyDecisionFilter();
applyGaps();
document.querySelectorAll('.journey').forEach(markHot);
