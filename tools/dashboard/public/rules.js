// Pure rules the page script follows, kept apart from the DOM so the tests
// run them without a browser.

// A card that just became current takes no primary action for this long, so a
// held or repeated Enter cannot answer a card the owner has not read.
export const READY_DELAY = 700;

/**
 * A one-card queue's guard (For you and the Stack each have one). `show(id)`
 * is called whenever the queue's card is what the owner sees: the queue's
 * view is on screen and the card is its current one; `show(null)` when it is
 * not (another view, or the list instead of the card). The reading wait
 * starts when the shown card changes, so coming back to a view, or `restart`,
 * starts it again. A key press is acted on only when it is not a repeat; the
 * primary action only once ready.
 *
 * `hold()` is called when a move or an answer starts and `release()` once the
 * next card is current (or the answer failed and the card stays): in between
 * every action key and action button is refused, so nothing lands on the
 * card that is leaving. A refresh in between does not end it.
 */
export function createGuard({ delay = READY_DELAY, now = () => Date.now() } = {}) {
  let shown = null;
  let readyAt = 0;
  let held = false;
  return {
    show(id, { restart = false } = {}) {
      if (id === shown && !restart) return false;
      shown = id;
      if (id === null) return false;
      readyAt = now() + delay;
      return true;
    },
    get shown() {
      return shown;
    },
    get held() {
      return held;
    },
    hold() {
      held = true;
    },
    release() {
      held = false;
    },
    remaining: () => Math.max(0, readyAt - now()),
    acceptsKey: (event) => !held && !event.repeat,
    acceptsAction: () => !held,
    acceptsPrimary: () => !held && now() >= readyAt,
  };
}

/**
 * What the owner is told when a note is refused (409): always a notice,
 * whether or not its box is still on the page, with the text when there is
 * one.
 */
export function noteRefusal({ text = '' } = {}) {
  return text.trim()
    ? { message: 'This part of the document changed; your note was not recorded. Your text:', text }
    : { message: 'This part of the document changed; your note was not recorded.', text: null };
}

/**
 * What the owner is told when a verdict is refused because the entry changed
 * or left the list (409 or 404), with the note when there is one.
 */
export function verdictRefusal({ text = '' } = {}) {
  const message = 'This entry changed or was removed; your verdict was not recorded.';
  return text.trim() ? { message: `${message} Your note:`, text } : { message, text: null };
}

// What the owner is told when a reply could not be sent and its box is gone.
export function replyLost(text) {
  return { message: 'This item was closed or answered meanwhile, so your text was not sent. Your text:', text };
}
