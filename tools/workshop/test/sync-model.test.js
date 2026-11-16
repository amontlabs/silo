// Model-based test of sync: random sequences of owner actions on GitHub,
// edits of the files, full syncs, syncs cut short by a rate limit and dry
// runs, checked after every step against a reference model of what the
// files and the owner's labels and states must be.
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import { test } from 'node:test';
import { listItems } from '../src/decisions.js';
import { startFake } from './fake-github.js';
import { run, sandbox } from './helpers.js';

function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const VERDICTS = [null, 'take', 'refine', 'later', 'drop'];
const HEADER = '# Decisions\n\nIntro line.\n\n';

test('sync keeps the files and the owner’s labels and states right under random operation sequences', async (t) => {
  const fake = await startFake();
  t.after(fake.stop);
  for (let seed = 1; seed <= 25; seed += 1) {
    const random = rng(seed);
    const pick = (list) => list[Math.floor(random() * list.length)];
    fake.issues.clear();
    fake.labels.length = 0;
    fake.events.length = 0;
    fake.limitAfterWrites = null;
    const unrelated = fake.addIssue({ title: 'Unrelated', body: '<!-- silo-entry: e1 -->', labels: ['bug'] });

    const model = {
      entries: [1, 2, 3].map((i) => ({ id: `e${i}`, name: `Entry ${i}`, what: `What ${i}.`, verdict: i === 1 ? 'take' : null })),
      decisions: ['2026-11-16 · owner · First decision.', `2026-11-15 · owner · ${'A long decision line '.repeat(8)}end.`],
      owner: new Map(), // issue number -> { verdict, state, reason } as the owner left it
    };
    const box = sandbox(t, { features: '', decisions: '' });
    const fileNumbers = () => new Map(box.features().split('\n').filter(Boolean).map((l) => JSON.parse(l)).filter((e) => e.issue).map((e) => [e.id, e.issue]));
    const writeFeatures = () => {
      const numbers = fileNumbers();
      const lines = model.entries.map((e, i) => {
        const base = `{"n": ${i + 1}, "id": ${JSON.stringify(e.id)}, "name": ${JSON.stringify(e.name)}, "what": ${JSON.stringify(e.what)}, "verdict": ${JSON.stringify(e.verdict)}`;
        return numbers.has(e.id) ? `${base}, "issue": ${numbers.get(e.id)}}` : `${base}}`;
      });
      writeFileSync(box.featuresPath, `${lines.join('\n')}\n`);
    };
    const decisionsText = () => box.decisions().replace(/ \(#\d+\)(?=\n)/g, '');
    const writeDecision = (text) => {
      model.decisions.unshift(text);
      const current = box.decisions();
      writeFileSync(box.decisionsPath, current.slice(0, HEADER.length) + `- ${text}\n` + current.slice(HEADER.length));
    };
    writeFeatures();
    writeFileSync(box.decisionsPath, `${HEADER}${model.decisions.map((d) => `- ${d}\n`).join('')}\nA closing paragraph.\n`);
    const expectedDecisions = () => `${HEADER}${model.decisions.map((d) => `- ${d}\n`).join('')}\nA closing paragraph.\n`;

    const checkAlways = (step) => {
      const entries = box.features().split('\n').filter(Boolean).map((l) => JSON.parse(l));
      assert.deepEqual(entries.map(({ issue, n, ...rest }) => rest), model.entries, `${step}: file fields`);
      const markers = [...fake.issues.values()].filter((i) => i.labels.includes('workshop:entry')).map((i) => i.body.match(/silo-entry: (\S+) -->/)[1]);
      assert.equal(new Set(markers).size, markers.length, `${step}: one issue per entry`);
      for (const e of entries.filter((x) => x.issue)) {
        assert.ok(fake.issues.get(e.issue).body.endsWith(`<!-- silo-entry: ${e.id} -->`), `${step}: number points at its entry`);
      }
      for (const [number, owned] of model.owner) {
        const issue = fake.issues.get(number);
        const labels = issue.labels.filter((l) => l.startsWith('verdict:'));
        assert.deepEqual(labels, owned.verdict ? [`verdict:${owned.verdict}`] : [], `${step}: verdict labels untouched`);
        assert.deepEqual([issue.state, issue.state_reason], [owned.state, owned.reason], `${step}: state untouched`);
      }
      assert.equal(decisionsText(), expectedDecisions(), `${step}: DECISIONS.md bytes`);
      const decisionBodies = [...fake.issues.values()].filter((i) => i.labels.includes('workshop:decision')).map((i) => i.body);
      assert.equal(new Set(decisionBodies).size, decisionBodies.length, `${step}: one issue per decision`);
      assert.deepEqual([unrelated.title, unrelated.labels, unrelated.state], ['Unrelated', ['bug'], 'open'], `${step}: other issues untouched`);
    };
    const checkComplete = (step) => {
      const numbers = fileNumbers();
      for (const e of model.entries) {
        const issue = fake.issues.get(numbers.get(e.id));
        assert.ok(issue, `${step}: ${e.id} has an issue`);
        assert.equal(issue.title, e.name, `${step}: title`);
        assert.equal(issue.body, `${e.what}\n\n<!-- silo-entry: ${e.id} -->`, `${step}: body`);
      }
      for (const item of listItems(box.decisions().split('\n'))) {
        const issue = fake.issues.get(item.issue);
        assert.ok(issue && issue.body === item.text && issue.state === 'closed' && issue.user.login === 'silo-workshop[bot]', `${step}: decision issue`);
      }
    };

    for (let stepIndex = 0; stepIndex < 14; stepIndex += 1) {
      const op = pick(['sync', 'sync', 'limited', 'concurrent', 'dry', 'verdict', 'state', 'edit', 'add', 'decide']);
      const step = `seed ${seed} step ${stepIndex} ${op}`;
      const numbered = [...fileNumbers().values()];
      if (op === 'sync') {
        const result = await run(['sync'], { fake, box });
        assert.equal(result.code, 0, `${step}: ${result.stderr}`);
        checkAlways(step);
        checkComplete(step);
        const again = await run(['sync'], { fake, box });
        assert.match(again.stdout, /^created 0, updated 0, unchanged \d+$/m, `${step}: idempotent`);
      } else if (op === 'limited') {
        fake.limitAfterWrites = fake.writes + Math.floor(random() * 6);
        const result = await run(['sync'], { fake, box });
        fake.limitAfterWrites = null;
        assert.ok(result.code === 0 || result.code === 1, step);
        checkAlways(step);
      } else if (op === 'concurrent') {
        // The files are edited while sync is writing to GitHub; the edits must survive.
        const at = 1 + Math.floor(random() * 4);
        let writes = 0;
        const fetch = async (url, init) => {
          const response = await globalThis.fetch(url, init);
          if (init.method !== 'GET' && (writes += 1) === at) {
            const entry = pick(model.entries);
            entry.what = `${entry.what} Edited meanwhile.`;
            writeFeatures();
            writeDecision(`2026-11-18 · owner · Added during step ${stepIndex} of seed ${seed}.`);
          }
          return response;
        };
        const result = await run(['sync'], { fake, box, overrides: { fetch } });
        assert.equal(result.code, 0, `${step}: ${result.stderr}`);
        checkAlways(step);
        const before = JSON.stringify([...fake.issues.values()]);
        const files = box.features() + box.decisions();
        assert.equal((await run(['sync', '--dry-run'], { fake, box })).code, 0, step);
        assert.equal(JSON.stringify([...fake.issues.values()]), before, `${step}: GitHub unchanged`);
        assert.equal(box.features() + box.decisions(), files, `${step}: files unchanged`);
      } else if (op === 'verdict' && numbered.length) {
        const number = pick(numbered);
        const verdict = pick(VERDICTS);
        fake.setVerdictLabel(number, verdict);
        const issue = fake.issues.get(number);
        model.owner.set(number, { verdict, state: issue.state, reason: issue.state_reason });
      } else if (op === 'state' && numbered.length) {
        const number = pick(numbered);
        const issue = fake.issues.get(number);
        if (issue.state === 'open') fake.close(number, pick(['completed', 'not_planned']));
        else Object.assign(issue, { state: 'open', state_reason: 'reopened', closed_at: null });
        const verdict = issue.labels.find((l) => l.startsWith('verdict:'))?.slice(8) ?? null;
        model.owner.set(number, { verdict, state: issue.state, reason: issue.state_reason });
      } else if (op === 'edit') {
        const entry = pick(model.entries);
        if (random() < 0.5) entry.name = `${entry.name}!`;
        else entry.what = `${entry.what} More "quoted" — text.`;
        writeFeatures();
      } else if (op === 'add') {
        model.entries.push({ id: `e${model.entries.length + 1}`, name: `Entry ${model.entries.length + 1}`, what: 'New.', verdict: null });
        writeFeatures();
      } else if (op === 'decide') {
        writeDecision(`2026-11-17 · owner · Decision ${stepIndex} of seed ${seed}.`);
      }
      checkAlways(step);
    }
    assert.equal((await run(['sync'], { fake, box })).code, 0);
    checkAlways(`seed ${seed} final`);
    checkComplete(`seed ${seed} final`);
  }
});
