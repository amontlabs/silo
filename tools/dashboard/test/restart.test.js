import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseFeatures, parseStages } from '../src/features.js';
import { carriedSummary, decisionTimeline, renderPage, verdictSummary } from '../src/views.js';
import { createServer } from '../src/server.js';
import { Store } from '../src/store.js';
import { RESTART, makeRepo, request, tempDir, writeDoc } from './helpers.js';

const NO_CHANGES = { changes: [], state: 'nogit', head: null };
const at = (day, hour = 12) => new Date(2026, 10, day, hour).toISOString();

const FEATURES = parseFeatures(
  [
    { n: 1, id: 'a', stage: 'shell', name: 'Name a', verdict: 'take' },
    { n: 2, id: 'b', stage: 'shell', name: 'Name b' },
    { n: 3, id: 'c', stage: 'shell', name: 'Name c' },
    { n: 4, id: 'd', stage: 'living', name: 'Name d', verdict: 'drop', why: 'You dropped this on [10 Nov](../DIRECTION.md).' },
    { n: 5, id: 'e', stage: 'living', name: 'Name e', verdict: 'take' },
  ]
    .map((entry) => JSON.stringify(entry))
    .join('\n'),
).features;
const STACK = (rows = []) => ({ features: FEATURES, stages: parseStages(null, FEATURES), skipped: 0, rows });

function item(id, fields, replies = []) {
  return { id, kind: 'decide', journey: null, title: `Item ${id}`, body: '', default_text: null, source: null, status: 'open', created_at: at(10), closed_at: null, close_note: null, options: null, anchor: null, anchor_text: null, attachments: [], state: 'waiting', ...fields, replies };
}
const reply = (id, day, fields) => ({ id, item_id: 0, author: 'owner', verdict: null, text: '', choice: null, choice_label: null, reaction: null, created_at: at(day), acked_at: null, ...fields });

function page({ items = [], stack = null, restart = null, decisions = null, product = null, changes = NO_CHANGES } = {}) {
  return renderPage({ items, docs: { product, decisions, restart }, changes, notes: [], stack, verdictHistory: stack?.rows ?? [] }).toString();
}
const section = (html, id) => new RegExp(`<section class="view" id="${id}"[\\s\\S]*?</section>`).exec(html)?.[0] ?? '';

// ---- where we are ----

test('the Stack starts with "Where we are": Done folded, Now in full, Next and Then muted and cut to their first sentence', () => {
  const html = page({ stack: STACK(), restart: RESTART });
  const stack = section(html, 'stack');
  const strip = /<nav class="where-we-are" id="where-we-are" aria-label="Where we are">[\s\S]*?<\/nav>/.exec(stack)[0];
  assert.ok(stack.indexOf('class="where-we-are"') < stack.indexOf('<p class="eyebrow">The stack</p>'), 'above the lede');
  assert.match(strip, /<details class="ww-done" id="where-done"><summary>2 done<\/summary><ol><li>the old tree is frozen\.<\/li><li>the feature list\.<\/li><\/ol><\/details>/);
  assert.doesNotMatch(strip, /<details class="ww-done" id="where-done" open/);
  assert.match(strip, /<div class="ww-step ww-now"><span class="ww-word">Now<\/span><span>the owner goes down the <strong>Stack<\/strong>\. The first entries are the starter\.<\/span><\/div>/);
  assert.match(strip, /<div class="ww-step ww-later"><span class="ww-word">Next<\/span><span>start <code>main<\/code> again\.<\/span><\/div>/);
  assert.match(strip, /<div class="ww-step ww-later"><span class="ww-word">Then<\/span><span>design the shell\.<\/span><\/div>/);
  assert.doesNotMatch(strip, /Wait for the owner|Show it first/);
  assert.ok(strip.indexOf('ww-now') < strip.indexOf('>Next<') && strip.indexOf('>Next<') < strip.indexOf('>Then<'));
});

test('"Where we are": odd first words show muted without a word; no Done, no chip; no file, heading or list, no strip', () => {
  const html = page({ stack: STACK(), restart: '## Where the work stands\n\n1. Later: a <b>bold</b> idea. More.\n2. NOW: go.\n' });
  const strip = /<nav class="where-we-are"[\s\S]*?<\/nav>/.exec(html)[0];
  assert.doesNotMatch(strip, /ww-done/);
  assert.match(strip, /<div class="ww-step ww-later"><span>Later: a &lt;b&gt;bold&lt;\/b&gt; idea\.<\/span><\/div>/);
  assert.match(strip, /ww-now"><span class="ww-word">Now<\/span><span>go\.<\/span>/);
  for (const restart of [null, '# Restart\n\n## Decided\n\n1. Done: x\n', '## Where the work stands\n\nNothing yet.\n']) {
    assert.doesNotMatch(page({ stack: STACK(), restart }), /where-we-are/, String(restart));
  }
  assert.doesNotMatch(page({ restart: RESTART }), /where-we-are/, 'no list: no Stack, no strip');
});

// ---- For you ----

test('For you: one column; empty, the tick and a button to the Stack; earlier items behind one closed disclosure', () => {
  let html = page({ stack: STACK() });
  let forYou = section(html, 'for-you');
  assert.match(forYou, /<div class="card empty"><div class="tick" aria-hidden="true">✓<\/div><h2>Nothing is waiting for you<\/h2><a class="btn go" href="#stack">Go to the Stack<\/a><\/div>/);
  assert.doesNotMatch(forYou, /<aside|Where the product is|Changed since you last looked|class="mini/);
  assert.doesNotMatch(forYou, /earlier-items/, 'nothing answered: no disclosure');
  assert.match(html, /data-view="for-you">For you<\/a>/, 'nothing waits: no badge');

  const answered = item(1, { state: 'answered', kind: 'look', default_text: 'd' }, [reply(1, 16, { verdict: 'agree' })]);
  const closed = item(2, { state: 'closed', status: 'closed', closed_at: at(15) });
  const waiting = item(3, {});
  html = page({ stack: STACK(), items: [answered, closed, waiting] });
  forYou = section(html, 'for-you');
  assert.match(html, /data-view="for-you">For you <span class="badge">1<\/span><\/a>/);
  assert.match(forYou, /<details class="earlier" id="earlier-items"><summary>Earlier items \(2\)<\/summary><div class="answered-list"><details class="done-row" id="item-1">[\s\S]*<details class="done-row" id="item-2">/);
  assert.ok(forYou.indexOf('data-card="3"') < forYou.indexOf('earlier-items'), 'below the queue');
  assert.doesNotMatch(forYou, /Go to the Stack/);
  // Without a list the empty state has no button.
  assert.doesNotMatch(page(), /Go to the Stack/);
});

test('a card names the list entry it is about: a link to the Stack when the entry is on the list, plain text otherwise', () => {
  const html = page({ stack: STACK(), items: [item(1, { journey: 'b' }), item(2, { journey: 'gone-entry' }), item(3, {})] });
  assert.match(html, /<article class="card" id="item-1"[\s\S]*?<span class="where">on <a href="#feature-b"><b>Name b<\/b><\/a><\/span>/);
  assert.match(html, /<article class="card" id="item-2"[\s\S]*?<span class="where">on <b>gone-entry<\/b><\/span>/);
  assert.doesNotMatch(/<article class="card" id="item-3"[\s\S]*?<\/article>/.exec(html)[0], /class="where"/);
  // Without the list, any entry is plain text.
  assert.match(page({ items: [item(1, { journey: 'b' })] }), /<span class="where">on <b>b<\/b><\/span>/);
});

// ---- Decisions ----

const DECISIONS = `# Decisions

- 2026-11-16 · owner · Start from an empty tree.
- 2026-11-15 · owner · The first user is the owner.
- 2026-11-16 · orchestrator · Merging changes main only.
- Not a dated line.
`;

function timelineFixture() {
  // Every row of the verdicts table: history, not only the newest per entry.
  const rows = [
    { id: 1, feature_id: 'a', name: 'Name a', verdict: 'take', note: '', created_at: at(16, 9) },
    { id: 2, feature_id: 'b', name: 'Name b', verdict: 'later', note: 'After the shell', created_at: at(16, 10) },
    { id: 4, feature_id: 'gone', name: 'Gone entry', verdict: 'take', note: '', created_at: at(16, 11) },
    { id: 5, feature_id: 'e', name: 'Name e', verdict: null, note: '', created_at: at(16, 12) },
    { id: 7, feature_id: 'c', name: 'Name c', verdict: 'drop', note: '', created_at: at(14) },
    { id: 8, feature_id: 'b', name: 'Name b', verdict: 'drop', note: 'Not needed', created_at: at(17) },
    { id: 9, feature_id: 'c', name: 'Name c', verdict: null, note: '', created_at: at(17, 13) },
  ];
  const look = item(10, { kind: 'look', default_text: 'Blue', title: 'Colour', state: 'answered' }, [reply(20, 17, { verdict: 'agree' }), { ...reply(21, 17, {}), author: 'agent', text: 'Thanks' }]);
  const decide = item(11, { title: 'Pick a name', state: 'answered', options: [{ label: 'Silo' }, { label: 'Box' }] }, [reply(22, 16, { choice: 0, choice_label: 'Silo', text: 'Silo' })]);
  const disagree = item(12, { kind: 'look', default_text: 'x', title: 'Sidebar', state: 'answered' }, [reply(23, 16, { verdict: 'disagree', text: 'By use' })]);
  const legacy = item(13, { kind: 'note', journey: 'live', title: 'T3 Code connects', anchor: 'journeys#live/3', anchor_text: 'T3 Code connects', state: 'answered' }, [reply(24, 15, { reaction: 'essential' })]);
  const note = item(14, { kind: 'note', title: 'The problem', anchor: 'product#the-problem', anchor_text: 'The problem', state: 'answered' }, [reply(25, 15, { text: 'Say it shorter' })]);
  return { rows, items: [look, decide, disagree, legacy, note] };
}

test('the timeline merges DECISIONS.md, every verdict row and answers by day, newest first; file-only verdicts go last', () => {
  const { rows, items } = timelineFixture();
  const timeline = decisionTimeline({ decisionsText: DECISIONS, features: FEATURES, verdictRows: rows, items });
  assert.deepEqual(
    timeline.days.map((day) => [day.date, day.decisions.map((entry) => entry.text), day.verdicts.map((entry) => [entry.id, entry.verdict, entry.previous, entry.previousFrom, entry.onList]), day.answers.map((answer) => answer.reply.id)]),
    [
      ['2026-11-17', [], [['b', 'drop', 'later', 'store', true], ['c', null, 'drop', 'store', true]], [20]],
      [
        '2026-11-16',
        ['Start from an empty tree.', 'Merging changes main only.'],
        [['a', 'take', 'take', 'file', true], ['b', 'later', null, null, true], ['gone', 'take', null, null, false], ['e', null, 'take', 'file', true]],
        [23, 22],
      ],
      ['2026-11-15', ['The first user is the owner.'], [], [25, 24]],
      ['2026-11-14', [], [['c', 'drop', null, null, true]], []],
    ],
  );
  // a and e have store rows; d's verdict comes only from the file, with its why.
  assert.deepEqual(timeline.carried.map((entry) => [entry.id, entry.verdict, entry.why]), [['d', 'drop', 'You dropped this on [10 Nov](../DIRECTION.md).']]);
  assert.deepEqual(timeline.other, ['Not a dated line.']);
  assert.equal(timeline.total, 4);
  assert.equal(verdictSummary([{ verdict: 'take' }, { verdict: 'take' }, { verdict: 'later' }, { verdict: 'drop' }, { verdict: 'drop' }, { verdict: 'refine' }, { verdict: null }]), 'You took 2 entries, asked to refine 1, kept 1 for later, dropped 2, cleared 1');
  assert.equal(verdictSummary([{ verdict: 'take' }]), 'You took 1 entry');
  assert.equal(carriedSummary([...Array(13).fill({ verdict: 'take' }), ...Array(3).fill({ verdict: 'drop' })]), '13 entries taken, 3 dropped, from decisions made before the Stack');
  // Nothing at all: an empty timeline.
  assert.deepEqual(decisionTimeline({ decisionsText: null, features: null, verdictRows: [], items: [] }), { days: [], other: [], carried: [], total: 0 });
});

test('Decisions page: one folded row per day of verdicts as history, linking to the Stack; carried-over verdicts never say "you"; answers link to their item', () => {
  const { rows, items } = timelineFixture();
  const html = page({ stack: STACK(rows), decisions: DECISIONS, items });
  const view = section(html, 'decisions');
  assert.deepEqual([...view.matchAll(/data-decision-kind="([a-z]+)"/g)].map((match) => match[1]), ['all', 'decision', 'verdict', 'answer']);
  assert.match(view, /data-total="4">4 decisions/);
  assert.match(html, /data-view="decisions">Decisions <span class="badge quiet">4<\/span>/, 'the tab counts DECISIONS.md lines only');
  const days = [...view.matchAll(/<time datetime="([0-9-]+)">/g)].map((match) => match[1]);
  assert.deepEqual(days, ['2026-11-17', '2026-11-16', '2026-11-15', '2026-11-14']);
  const fold = /<details class="dec fold" data-kind="verdict" id="verdicts-2026-11-16">[\s\S]*?<\/details>/.exec(view)[0];
  assert.match(fold, /<span class="rkind verdict">Verdict<\/span><span class="who you">you<\/span><span class="what">You took 2 entries, kept 1 for later, cleared 1<\/span>/);
  assert.match(fold, /<li><a href="#feature-a">Name a<\/a><span class="vchip small v-take">Taken<\/span><span class="v-change">confirmed<\/span><\/li>/);
  assert.match(fold, /<li><a href="#feature-b">Name b<\/a><span class="vchip small v-later">Later<\/span><span class="fold-note">After the shell<\/span><\/li>/);
  assert.match(fold, /<li><span>Gone entry<\/span><span class="vchip small v-take">Taken<\/span><\/li>/, 'an entry no longer on the list has no link');
  assert.match(fold, /<li><a href="#feature-e">Name e<\/a><s class="v-was"><span class="vchip small v-take">Taken<\/span><\/s><span class="v-change">cleared its verdict<\/span><\/li>/);
  // A later change and a later clear show on their own day.
  const later = /<details class="dec fold" data-kind="verdict" id="verdicts-2026-11-17">[\s\S]*?<\/details>/.exec(view)[0];
  assert.match(later, /You dropped 1, cleared 1/);
  assert.match(later, /<li><a href="#feature-b">Name b<\/a><s class="v-was"><span class="vchip small v-later">Later<\/span><\/s><span class="v-change">changed to dropped<\/span><span class="vchip small v-drop">Dropped<\/span><span class="fold-note">Not needed<\/span><\/li>/);
  assert.match(later, /<a href="#feature-c">Name c<\/a><s class="v-was"><span class="vchip small v-drop">Dropped<\/span><\/s><span class="v-change">cleared its verdict/);
  assert.match(view, /id="verdicts-2026-11-14">[\s\S]*?You dropped 1[\s\S]*?<a href="#feature-c">Name c<\/a><span class="vchip small v-drop">Dropped<\/span><\/li>/, 'the earlier verdict stays on its day');
  // Carried over: not the owner's.
  const carried = /<div class="day carried">[\s\S]*?<\/details><\/div><\/div>/.exec(view)[0];
  assert.match(carried, /<time>Carried over from earlier decisions<\/time><div><details class="dec fold" data-kind="verdict" id="verdicts-carried">\s*<summary><span class="rkind list">From the list<\/span><span class="who carried">carried over<\/span><span class="what">1 dropped, from decisions made before the Stack<\/span><\/summary>/);
  assert.match(carried, /<li><a href="#feature-d">Name d<\/a><span class="vchip small v-drop">Dropped<\/span><span class="fold-note">You dropped this on 10 Nov\.<\/span><\/li>/, 'its why, with the relative link as text');
  assert.doesNotMatch(carried, /who you|>you<|You took|You dropped 1/);
  assert.ok(view.indexOf('verdicts-carried') > view.indexOf('<time>Other</time>'), 'carried over is the last group');
  // Answers: what was answered, linking to the item.
  assert.match(view, /<div class="dec" data-kind="answer"><span class="rkind answer">Answer<\/span><span class="who you">you<\/span><span class="what"><a href="#item-10">Colour<\/a> · Agreed<\/span><\/div>/);
  assert.match(view, /<a href="#item-11">Pick a name<\/a> · Chose “Silo”/);
  assert.match(view, /<a href="#item-12">Sidebar<\/a> · Disagreed: By use/);
  assert.match(view, /<a href="#product-the-problem">The problem<\/a> · Note: Say it shorter|<a href="#orphan-product-the-problem">The problem<\/a> · Note: Say it shorter/);
  // A journey note from an earlier version is kept, by its anchor text, without a link.
  assert.match(view, /<span class="what"><b>T3 Code connects<\/b> · Marked essential<\/span>/);
  assert.doesNotMatch(view, /journeys#|href="#journey/);
  // Decisions keep their who and their kind.
  assert.match(view, /<div class="dec" data-kind="decision"><span class="rkind decision">Decision<\/span><span class="who you">you<\/span><span class="what">Start from an empty tree\.<\/span><\/div>/);
  assert.match(view, /<span class="who ">orchestrator<\/span>/);
});

test('the Stack: a verdict from the file shows as carried over with Confirm; the owner\'s own verdict does not', () => {
  const html = page({ stack: STACK([{ id: 3, feature_id: 'e', name: 'Name e', verdict: 'later', note: '', created_at: at(16) }]) });
  const controls = (id) => {
    const card = html.slice(html.indexOf(`data-entry="${id}"`));
    return card.slice(card.indexOf('<div class="controls'), card.indexOf('</article>'));
  };
  assert.match(controls('a'), /<div class="verdict-now carried"><span class="vchip v-take">Taken<\/span><span class="v-from">carried over<\/span>/);
  assert.match(controls('a'), /<span class="v-acts"><button class="btn small go" type="button" data-pick="take">Confirm<\/button><button class="btn small" type="button" data-change/);
  assert.match(controls('d'), /data-pick="drop">Confirm<\/button>/);
  // e's store row wins over the file: the owner's own verdict, no Confirm.
  assert.match(controls('e'), /<div class="verdict-now "><span class="vchip v-later">Later<\/span>/);
  assert.doesNotMatch(controls('e'), /carried over|Confirm/);
  assert.doesNotMatch(controls('b'), /verdict-now|Confirm/, 'undecided: no verdict shown');
});

test('Decisions page: an answer to a waiting item links to its card; text is escaped and kept to one line', () => {
  const waiting = item(30, { title: '<b>Q</b>' }, [{ ...reply(40, 16, { text: 'line one\n\n<script>x</script> ' + 'w'.repeat(300) }) }]);
  const view = section(page({ items: [waiting] }), 'decisions');
  assert.match(view, /<a href="#item-30">&lt;b&gt;Q&lt;\/b&gt;<\/a> · line one &lt;script&gt;x&lt;\/script&gt; w+…<\/span>/);
  assert.doesNotMatch(view, /<script>|<b>Q/);
  assert.doesNotMatch(view, /Nothing decided yet/);
  assert.match(section(page(), 'decisions'), /Nothing decided yet\./);
});

function rng(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

test('model: the timeline shows every verdict row once, on its day, with the verdict it replaced; carried over only without any row; every owner reply once', () => {
  const ids = [...FEATURES.map((feature) => feature.id), 'gone-1', 'gone-2'];
  const fileVerdict = new Map(FEATURES.map((feature) => [feature.id, feature.verdict]));
  for (const seed of [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]) {
    const random = rng(seed);
    const pick = (list) => list[Math.floor(random() * list.length)];
    const rows = [];
    const items = [];
    let replyId = 0;
    for (let step = 0; step < 80; step += 1) {
      const op = pick(['verdict', 'verdict', 'clear', 'item', 'reply', 'agent']);
      if (op === 'verdict' || op === 'clear') {
        const id = pick(ids);
        rows.push({ id: rows.length + 1, feature_id: id, name: `Stored ${id}`, verdict: op === 'clear' ? null : pick(['take', 'refine', 'later', 'drop']), note: '', created_at: at(1 + Math.floor(random() * 20), Math.floor(random() * 24)) });
      } else if (op === 'item' || !items.length) {
        items.push(item(items.length + 1, { kind: pick(['look', 'decide', 'shipped', 'note']), state: 'answered' }));
      } else {
        replyId += 1;
        pick(items).replies.push({ ...reply(replyId, 1 + Math.floor(random() * 20), { text: `r${replyId}` }), author: op === 'agent' ? 'agent' : 'owner' });
      }
      // The rows reach the timeline in any order; history follows row ids.
      const shuffled = [...rows].sort(() => random() - 0.5);
      const timeline = decisionTimeline({ decisionsText: null, features: FEATURES, verdictRows: shuffled, items });
      // Expected: each row on the local day of its time, replacing the entry's row before it, else the file's verdict.
      const expected = new Map();
      const last = new Map();
      for (const row of rows) {
        const prior = last.get(row.feature_id);
        const previous = prior ? prior.verdict : (fileVerdict.get(row.feature_id) ?? null);
        expected.set(row.id, [row.feature_id, row.verdict, previous, new Date(row.created_at).getDate()]);
        last.set(row.feature_id, row);
      }
      const shown = new Map();
      for (const day of timeline.days) {
        for (const entry of day.verdicts) {
          assert.ok(!shown.has(entry.row), `seed ${seed} step ${step}: row ${entry.row} shown twice`);
          shown.set(entry.row, [entry.id, entry.verdict, entry.previous, Number(day.date.slice(8))]);
        }
      }
      assert.deepEqual(shown, expected, `seed ${seed} step ${step}: verdict history`);
      const carried = ids.filter((id) => fileVerdict.get(id) && !last.has(id));
      assert.deepEqual(timeline.carried.map((entry) => entry.id), carried, `seed ${seed} step ${step}: carried over`);
      const answers = timeline.days.flatMap((day) => day.answers.map((answer) => answer.reply.id)).sort((a, b) => a - b);
      const owner = items.flatMap((entry) => entry.replies.filter((r) => r.author === 'owner').map((r) => r.id)).sort((a, b) => a - b);
      assert.deepEqual(answers, owner, `seed ${seed} step ${step}: answers`);
      const dates = timeline.days.map((day) => day.date);
      assert.deepEqual(dates, [...dates].sort().reverse(), 'newest day first');
    }
  }
});

test('repository Markdown keeps only absolute links: relative files and anchors render as their text', () => {
  const links = 'See [the list](features.jsonl), [up](../DIRECTION.md), [here](#x), ![shot](a.png), [site](https://example.com/a) and [mail](mailto:o@example.com).';
  const html = page({
    stack: STACK(),
    restart: `## Where the work stands\n\n1. Done: ${links}\n2. Now: ${links}\n`,
    decisions: `# Decisions\n\n- 2026-11-16 · owner · ${links}\n- Other: ${links}\n`,
    product: `# Silo\n\n## In one sentence\n\n${links}\n\n## Tile\n\n${links}\n`,
  });
  const regions = [/<nav class="where-we-are"[\s\S]*?<\/nav>/.exec(html)[0], section(html, 'decisions'), section(html, 'product').replace(/<div class="loop">[\s\S]*?<\/svg><\/div>/, '')];
  for (const region of regions) {
    assert.doesNotMatch(region, /href="(?:features\.jsonl|\.\.\/DIRECTION\.md|#x)"|<img/, region.slice(0, 40));
    assert.match(region, /See the list, up, here, shot, <a href="https:\/\/example\.com\/a">site<\/a> and <a href="mailto:o@example\.com">mail<\/a>\./);
  }
  // The Stack card's what and why too.
  const one = [{ ...FEATURES[1], what: links, why: links }];
  const card = page({ stack: { features: one, stages: parseStages(null, one), skipped: 0, rows: [] } });
  assert.doesNotMatch(section(card, 'stack'), /href="(?:features\.jsonl|\.\.\/DIRECTION\.md|#x)"/);
  // Item bodies are not repository Markdown: their relative links (attachments) stay.
  assert.match(page({ items: [item(1, { body: '[clip](/attachments/3/clip.mp4)' })] }), /<a href="\/attachments\/3\/clip\.mp4">clip<\/a>/);
});

test('the server shows every verdict row as history: a change and a clear stay on their day', async () => {
  const repo = makeRepo();
  writeDoc(repo, 'features.jsonl', `${JSON.stringify({ n: 1, id: 'one', stage: 'shell', name: 'Entry one' })}\n`);
  const store = new Store(tempDir());
  const server = createServer({ store, repoRoot: repo });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const first = store.addVerdict('one', { verdict: 'take', shown: 0, name: 'Entry one' });
    const second = store.addVerdict('one', { verdict: 'drop', shown: first, name: 'Entry one' });
    store.addVerdict('one', { verdict: null, shown: second, name: 'Entry one' });
    const view = section((await request(server.address().port)).body.toString(), 'decisions');
    assert.match(view, /You took 1 entry, dropped 1, cleared 1/);
    assert.match(view, /changed to dropped/);
    assert.match(view, /cleared its verdict/);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    store.close();
  }
});
