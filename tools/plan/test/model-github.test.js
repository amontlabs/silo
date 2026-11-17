// The verdict's one writer, on every path: random sequences of clicks on GitHub (by people who can
// decide and people who cannot), `plan decide`, `plan pull --write`, `plan sync` and edits of the
// plan, checked after every step against a reference model of the rules.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { basePlan, git } from './helpers.js';
import { world } from './github-helpers.js';
import { labelVerdicts } from '../model/comments.js';
import { readSyncedMarker } from '../model/markers.js';

const IDS = ['core', 'cli', 'fork'];
const VERDICTS = ['take', 'later', 'drop'];
const ACTORS = ['owner', 'maintainer', 'helper']; // helper has triage only

for (const seed0 of [11, 29, 47, 83]) {
  test(`model: random sequence ${seed0}`, async (t) => {
    let seed = seed0;
    // mulberry32: every choice of every operation is reached.
    const random = (n) => {
      seed = (seed + 0x6d2b79f5) | 0;
      let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
      return ((x ^ (x >>> 14)) >>> 0) % n;
    };
    const w = await world(t, basePlan());
    assert.equal((await w.run(['sync'])).code, 0);
    // The model: per feature, the labels, whether each label's newest `labeled` counts, whether the
    // newest verdict event is a counted removal, whether any verdict event exists, the file's verdict.
    const model = Object.fromEntries(IDS.map((id) => [id, { labels: new Set(), counted: {}, removalCounts: false, any: false, file: null }]));
    const counts = (actor) => actor !== 'helper';
    const commitAll = (message) => {
      git(w.root, 'add', '-A');
      if (git(w.root, 'status', '--porcelain').trim()) git(w.root, 'commit', '-q', '-m', message);
      git(w.root, 'push', '-q', 'origin', 'HEAD:main');
    };

    for (let step = 0; step < 30; step += 1) {
      const id = IDS[random(IDS.length)];
      const m = model[id];
      const number = w.fake.entry(id).number;
      const op = random(6);
      if (op === 0) {
        const verdict = VERDICTS[random(3)];
        const actor = ACTORS[random(3)];
        if (!m.labels.has(verdict)) {
          w.fake.label(number, `verdict:${verdict}`, actor);
          m.labels.add(verdict);
          m.counted[verdict] = counts(actor);
          m.any = true;
          m.removalCounts = false;
        }
      } else if (op === 1 && m.labels.size) {
        const verdict = [...m.labels][random(m.labels.size)];
        const actor = ACTORS[random(3)];
        w.fake.unlabel(number, `verdict:${verdict}`, actor);
        m.labels.delete(verdict);
        m.removalCounts = counts(actor);
        m.any = true;
      } else if (op === 2) {
        // Half the time a verdict GitHub shows already: the paths that must change nothing.
        const verdict = m.labels.size && random(2) ? [...m.labels][random(m.labels.size)] : [...VERDICTS, 'clear'][random(4)];
        const result = await w.run(['decide', id, verdict]);
        // The file at origin/main is the model's file: every pull is committed and pushed.
        const refused = verdict !== 'clear' && m.labels.has(verdict) && !m.counted[verdict];
        assert.equal(result.code, refused ? 1 : 0, result.err);
        const wanted = verdict === 'clear' ? [] : [verdict];
        const same = m.labels.size === wanted.length && wanted.every((v) => m.labels.has(v));
        if (refused) {
          // The label set by someone who cannot decide stays; nothing changes.
        } else if (!same || (verdict === 'clear' && m.file !== null)) {
          // A clear with no label but a verdict in the file puts the file's label on, then off.
          if (verdict !== 'clear' && !m.labels.has(verdict)) m.counted[verdict] = true;
          m.labels = new Set(wanted);
          m.any = true;
          m.removalCounts = true; // any removal in it is the bot's
        }
      } else if (op === 3) {
        assert.equal((await w.run(['pull', '--write'])).code, 0);
        for (const mid of IDS) {
          const f = model[mid];
          if (f.labels.size === 1) {
            const [label] = f.labels;
            if (label !== f.file && f.counted[label]) f.file = label;
          } else if (f.labels.size === 0 && f.file !== null && (!f.any || f.removalCounts)) f.file = null;
        }
        commitAll(`Owner: step ${step}`);
      } else if (op === 4) {
        assert.equal((await w.run(['sync'])).code, 0);
        const snapshot = w.fake.snapshot();
        assert.equal((await w.run(['sync'])).code, 0);
        assert.equal(w.fake.snapshot(), snapshot, 'a second sync changes nothing');
        for (const mid of IDS) assert.equal(readSyncedMarker(w.fake.entry(mid).body)?.length, 40);
      } else {
        w.change({ [id]: { what: `Step ${step}.` } });
      }

      // After every step: the files hold the model's verdicts; GitHub holds the model's labels; the
      // bot never set a verdict label except through decide; one issue per feature.
      for (const mid of IDS) {
        assert.equal(w.feature(mid).verdict ?? null, model[mid].file, `${mid} file at step ${step}`);
        assert.deepEqual(labelVerdicts(w.fake.entry(mid).labels), VERDICTS.filter((v) => model[mid].labels.has(v)), `${mid} labels at step ${step}`);
      }
      assert.equal([...w.fake.issues.values()].filter((i) => i.labels.includes('workshop:entry')).length, 4);
    }
  });
}
