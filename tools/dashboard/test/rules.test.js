import { test } from 'node:test';
import assert from 'node:assert/strict';
import { READY_DELAY, createGuard, noteRefusal, replyLost } from '../public/rules.js';

// A model of the For you queue driven by key events and a fake clock, the way
// app.js uses the guard: a key is acted on only if accepted, Enter runs the
// primary action only once the card is ready, and answering shows the next
// card after the leave animation.
function queue(cards) {
  let t = 0;
  const guard = createGuard({ now: () => t });
  const answered = [];
  let at = 0;
  guard.show(cards[at]);
  return {
    answered,
    get current() {
      return cards[at];
    },
    advance(ms) {
      t += ms;
    },
    enter({ repeat = false } = {}) {
      if (!guard.acceptsKey({ repeat })) return;
      if (!guard.acceptsPrimary()) return;
      answered.push(cards[at]);
      t += 250; // the card leaves; the next one becomes current
      at += 1;
      guard.show(at < cards.length ? cards[at] : null);
    },
    guard,
  };
}

test('holding Enter across three cards answers exactly one', () => {
  const q = queue([1, 2, 3]);
  q.advance(READY_DELAY);
  q.enter();
  for (let n = 0; n < 40; n += 1) {
    q.advance(100);
    q.enter({ repeat: true });
  }
  assert.deepEqual(q.answered, [1]);
  assert.equal(q.current, 2);
});

test('two quick presses answer only the first card; the next waits READY_DELAY', () => {
  const q = queue([1, 2, 3]);
  q.advance(READY_DELAY);
  q.enter();
  q.advance(50);
  q.enter();
  assert.deepEqual(q.answered, [1], 'the second press came before card 2 was ready');
  assert.ok(q.guard.remaining() > 0);
  q.advance(READY_DELAY);
  q.enter();
  assert.deepEqual(q.answered, [1, 2]);
});

test('the guard: a new card starts the wait, the same card again does not, no card ends it', () => {
  let t = 1000;
  const guard = createGuard({ now: () => t });
  assert.equal(guard.acceptsPrimary(), true);
  assert.equal(guard.show(7), true);
  assert.equal(guard.acceptsPrimary(), false);
  assert.equal(guard.remaining(), READY_DELAY);
  t += 400;
  assert.equal(guard.show(7), false, 'a refresh that keeps the card keeps the wait left');
  assert.equal(guard.remaining(), READY_DELAY - 400);
  t += READY_DELAY - 400;
  assert.equal(guard.acceptsPrimary(), true);
  assert.equal(guard.show(null), false);
  assert.equal(guard.shown, null);
  assert.equal(guard.acceptsKey({ repeat: true }), false);
  assert.equal(guard.acceptsKey({ repeat: false }), true);
});

test('a refused note or reaction is always told, with the text when there is one', () => {
  assert.deepEqual(noteRefusal({ text: 'About atelier' }), {
    message: 'This part of the document changed; your note was not recorded. Your text:',
    text: 'About atelier',
  });
  assert.deepEqual(noteRefusal({}), { message: 'This part of the document changed; your reaction was not recorded.', text: null });
  assert.equal(noteRefusal({ text: '  ' }).text, null);
  assert.equal(replyLost('x').text, 'x');
});
