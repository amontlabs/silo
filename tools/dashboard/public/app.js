// Forms post with fetch, then the affected regions of the page are replaced
// with the server's fresh HTML. Everything else is plain server-rendered HTML.

const REGIONS_AFTER_REPLY = ['nav', 'for-you', 'journeys', 'history'];
const ALL_REGIONS = ['nav', 'for-you', 'product', 'journeys', 'decisions', 'private', 'history'];

function openDetailIds() {
  return [...document.querySelectorAll('details[id][open]')].map((el) => el.id);
}

function drafts() {
  const saved = new Map();
  document.querySelectorAll('form[data-api] textarea').forEach((area) => {
    const form = area.closest('form');
    if (area.value && form.dataset.api) saved.set(`${form.dataset.api}|${form.dataset.verdict || ''}`, area.value);
  });
  return saved;
}

async function refresh(regions) {
  const open = openDetailIds();
  const saved = drafts();
  const response = await fetch('/', { headers: { Accept: 'text/html' }, cache: 'no-store' });
  if (!response.ok) throw new Error(`refresh failed (${response.status})`);
  const next = new DOMParser().parseFromString(await response.text(), 'text/html');
  for (const name of regions) {
    const fresh = next.querySelector(`[data-region="${name}"]`);
    const current = document.querySelector(`[data-region="${name}"]`);
    if (fresh && current) current.replaceWith(fresh);
  }
  for (const id of open) document.getElementById(id)?.setAttribute('open', '');
  document.querySelectorAll('form[data-api]').forEach((form) => {
    const text = saved.get(`${form.dataset.api}|${form.dataset.verdict || ''}`);
    if (text) form.querySelector('textarea').value = text;
  });
  applyDecisionFilter();
  renderMermaid();
  watchSections();
}

async function send(url, body) {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
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

const CHANGED_NOTE = 'This changed while you were reading. Read it again, then answer.';

function showError(origin, message) {
  const holder = origin.closest('form') || origin.closest('[data-controls]');
  const slot = holder?.querySelector('.form-error');
  if (slot) slot.textContent = message;
  else window.alert(message);
}

// Sends one owner action from a control inside a [data-controls] wrapper. The
// wrapper's data-after (the newest reply the page showed) goes with it; when
// the item changed since, the server answers 409 and the page shows it fresh.
async function act(origin, url, body, regions) {
  const controls = origin.closest('[data-controls]');
  const item = controls.dataset.item;
  if (controls.dataset.after !== undefined) body.after = Number(controls.dataset.after);
  try {
    await send(url, body);
    return true;
  } catch (error) {
    if (error.status !== 409 || !item) throw error;
    await refresh(regions);
    document.querySelectorAll(`[data-controls][data-item="${item}"] > .form-error`).forEach((slot) => {
      slot.textContent = CHANGED_NOTE;
    });
    return false;
  }
}

document.addEventListener('submit', async (event) => {
  const form = event.target.closest('form[data-api]');
  if (!form || !form.closest('[data-controls]')) return;
  event.preventDefault();
  const button = form.querySelector('button[type="submit"]');
  button.disabled = true;
  try {
    const body = { text: form.querySelector('textarea').value };
    if (form.dataset.verdict) body.verdict = form.dataset.verdict;
    if (await act(form, form.dataset.api, body, REGIONS_AFTER_REPLY)) {
      form.querySelector('textarea').value = '';
      await refresh(REGIONS_AFTER_REPLY);
    }
  } catch (error) {
    showError(form, error.message);
    button.disabled = false;
  }
});

document.addEventListener('click', async (event) => {
  const button = event.target.closest('button[data-post]');
  if (!button || !button.closest('[data-controls]')) return;
  button.disabled = true;
  const regions = button.dataset.refresh === 'all' ? ALL_REGIONS : REGIONS_AFTER_REPLY;
  try {
    if (await act(button, button.dataset.post, JSON.parse(button.dataset.body || '{}'), regions)) await refresh(regions);
  } catch (error) {
    showError(button, error.message);
    button.disabled = false;
  }
});

// Decisions: filter the entries of the list as you type.
function applyDecisionFilter() {
  const input = document.getElementById('decision-filter');
  const counter = document.getElementById('decision-count');
  if (!input || !counter) return;
  const needle = input.value.trim().toLowerCase();
  const entries = document.querySelectorAll('#decision-list .prose > ul > li, #decision-list .prose > ol > li');
  let shown = 0;
  entries.forEach((entry) => {
    const match = !needle || entry.textContent.toLowerCase().includes(needle);
    entry.hidden = !match;
    if (match) shown += 1;
  });
  document.querySelectorAll('#decision-list .doc-section').forEach((section) => {
    const own = section.querySelectorAll('.prose > ul > li, .prose > ol > li');
    section.hidden = Boolean(needle) && own.length > 0 && [...own].every((entry) => entry.hidden);
  });
  const total = Number(counter.dataset.total);
  counter.textContent = needle ? `${shown} of ${total}` : `${total} decision${total === 1 ? '' : 's'}`;
}

document.addEventListener('input', (event) => {
  if (event.target.id === 'decision-filter') applyDecisionFilter();
});

// Diagrams: mermaid loads only when a page has a mermaid block.
let mermaidLoading = null;
async function renderMermaid() {
  const blocks = [...document.querySelectorAll('pre.mermaid:not([data-processed])')];
  if (!blocks.length) return;
  mermaidLoading ||= import('/vendor/mermaid/mermaid.esm.min.mjs').then((module) => {
    const mermaid = module.default;
    const dark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: dark ? 'dark' : 'default', fontFamily: 'inherit' });
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

// Highlight the section being read in the top bar.
let activeRegion = null;
function markActive() {
  document.querySelectorAll('#nav a').forEach((link) => link.classList.toggle('active', link.getAttribute('href') === `#${activeRegion}`));
}
const spy = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) activeRegion = entry.target.dataset.region;
    }
    markActive();
  },
  { rootMargin: '-30% 0px -60% 0px' },
);
function watchSections() {
  document.querySelectorAll('section[data-region]').forEach((section) => spy.observe(section));
  markActive();
}

applyDecisionFilter();
renderMermaid();
watchSections();
