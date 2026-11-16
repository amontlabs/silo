// Pure rules the page script follows, kept apart from the DOM so the tests
// run them without a browser.

// A card that just became current takes no primary action for this long, so a
// held or repeated Enter cannot answer a card the owner has not read.
export const READY_DELAY = 700;

/**
 * The For you queue's guard. `show(id)` is called whenever the queue shows a
 * card and starts the wait when the card is a different one. A key press is
 * acted on only when it is not a repeat; the primary action only once ready.
 */
export function createGuard({ delay = READY_DELAY, now = () => Date.now() } = {}) {
  let shown = null;
  let readyAt = 0;
  return {
    show(id) {
      if (id === shown) return false;
      shown = id;
      if (id === null) return false;
      readyAt = now() + delay;
      return true;
    },
    get shown() {
      return shown;
    },
    remaining: () => Math.max(0, readyAt - now()),
    acceptsKey: (event) => !event.repeat,
    acceptsPrimary: () => now() >= readyAt,
  };
}

/**
 * What the owner is told when a note or reaction is refused (409): always a
 * notice, whether or not its box is still on the page, with the text when
 * there is one.
 */
export function noteRefusal({ text = '' } = {}) {
  return text.trim()
    ? { message: 'This part of the document changed; your note was not recorded. Your text:', text }
    : { message: 'This part of the document changed; your reaction was not recorded.', text: null };
}

// What the owner is told when a reply could not be sent and its box is gone.
export function replyLost(text) {
  return { message: 'This item was closed or answered meanwhile, so your text was not sent. Your text:', text };
}
