// The home page's pictures: for now only the cards. This starts them and says when they may move.
//
// Only one thing moves on its own at a time: the cards take turns while the tab is shown. A card the visitor points
// at or presses plays at once. A hidden tab holds everything. With reduced motion nothing moves until the visitor
// presses a card.

import { mountCards, type Cards } from './cards';

declare global {
  interface Window {
    /** The head script's timer that shows the stills if this script has not started in time. */
    siloPicturesFallback?: number;
  }
}

export function mountPictures(): { cards: Cards } {
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  let cards: Cards | undefined;
  const settle = () => {
    if (!cards) return;
    cards.turns(document.visibilityState !== 'hidden');
  };
  cards = mountCards(document.getElementById('cards')!, { still, onChange: settle });

  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') cards.rest();
    settle();
  });

  // Every picture is drawn: they can show.
  window.clearTimeout(window.siloPicturesFallback);
  document.documentElement.classList.remove('pictures-pending');
  settle();
  return { cards };
}
