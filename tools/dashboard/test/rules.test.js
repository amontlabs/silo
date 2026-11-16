import { test } from 'node:test';
import assert from 'node:assert/strict';
import { READY_DELAY, createGuard, noteRefusal, replyLost, verdictRefusal } from '../public/rules.js';

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

// The page as it uses the guard around a move or an answer: hold() when it
// starts, the leave animation, then show(next) once the next card is current.
function leavingQueue(cards) {
  let t = 0;
  const guard = createGuard({ now: () => t });
  let at = 0;
  const taken = [];
  let leaving = null;
  guard.show(cards[at]);
  const press = (event = {}) => guard.acceptsKey(event);
  return {
    guard,
    taken,
    advance(ms) {
      t += ms;
      if (leaving && t >= leaving.until) {
        at = leaving.to;
        leaving = null;
        guard.show(cards[at]);
        guard.release();
      }
    },
    get current() {
      return cards[at];
    },
    // Enter: the primary action on the current card, which then leaves.
    enter() {
      if (!press() || !guard.acceptsPrimary()) return;
      taken.push(cards[at]);
      guard.hold();
      leaving = { until: t + 250, to: at + 1 };
    },
    // →: the card leaves without an answer.
    skip() {
      if (!press()) return;
      guard.hold();
      leaving = { until: t + 200, to: at + 1 };
    },
    click() {
      return guard.acceptsAction();
    },
  };
}

test('during a move or an answer every action key and button is refused until the next card is current', () => {
  const q = leavingQueue(['a', 'b', 'c']);
  q.advance(READY_DELAY);
  q.skip();
  // Enter and clicks while "a" is leaving do nothing, also long after the reading wait.
  q.advance(150);
  q.enter();
  assert.equal(q.click(), false);
  assert.deepEqual(q.taken, []);
  q.advance(50);
  assert.equal(q.current, 'b');
  assert.equal(q.click(), true);
  q.enter();
  assert.deepEqual(q.taken, [], 'b has just appeared: its reading wait runs');
  q.advance(READY_DELAY);
  q.enter();
  q.advance(100);
  q.skip();
  q.enter();
  assert.deepEqual(q.taken, ['b'], 'nothing lands on b while it leaves');
  q.advance(200);
  assert.equal(q.current, 'c');
});

test('a held guard is released when the answer fails, and the card keeps its state', () => {
  let t = 0;
  const guard = createGuard({ now: () => t });
  guard.show(1);
  t += READY_DELAY;
  guard.hold();
  assert.equal(guard.acceptsPrimary(), false);
  assert.equal(guard.acceptsKey({ repeat: false }), false);
  guard.release();
  assert.equal(guard.acceptsPrimary(), true);
  // A refresh during the leave (show of the same or of the next card) keeps the hold.
  guard.hold();
  assert.equal(guard.show(1), false);
  guard.show(2);
  assert.equal(guard.acceptsAction(), false);
  guard.release();
  assert.equal(guard.acceptsAction(), true);
});

test('the reading wait starts when the card is seen: hidden views, coming back and restart', () => {
  let t = 0;
  const guard = createGuard({ now: () => t });
  // The page loads on another view: the queue's card is not seen, nothing starts.
  guard.show(null);
  t += 5000;
  // The owner opens the view: the wait starts now.
  assert.equal(guard.show('feature:a:1'), true);
  assert.equal(guard.acceptsPrimary(), false);
  t += READY_DELAY;
  assert.equal(guard.acceptsPrimary(), true);
  // Away and back to the same card: it waits again.
  guard.show(null);
  t += 100;
  assert.equal(guard.show('feature:a:1'), true);
  assert.equal(guard.acceptsPrimary(), false);
  t += READY_DELAY;
  // A stage jump or a See all click onto the card on screen restarts it.
  assert.equal(guard.show('feature:a:1', { restart: true }), true);
  assert.equal(guard.acceptsPrimary(), false);
  t += READY_DELAY;
  // The same entry with new text is a different key: it waits again.
  assert.equal(guard.show('feature:a:2'), true);
  assert.equal(guard.acceptsPrimary(), false);
});

test('a refused verdict is always told, with the note when there is one', () => {
  assert.deepEqual(verdictRefusal({ text: 'Smaller' }), { message: 'This entry changed or was removed; your verdict was not recorded. Your note:', text: 'Smaller' });
  assert.deepEqual(verdictRefusal({}), { message: 'This entry changed or was removed; your verdict was not recorded.', text: null });
});
