// The home page has two pictures: the film and the cards. This starts both and says which may move.
//
// Only one thing moves on its own at a time. The film plays while at least 30% of it is in view, and the cards
// take turns only while the film is not playing. A card the visitor points at or presses holds the film until
// they leave it or it ends. A hidden tab holds everything. With reduced motion nothing moves until the visitor
// presses Play or a card.

import { mountCards, type Cards } from './cards';
import { mountFilm, type Film } from './film/film';

declare global {
  interface Window {
    /** The head script's timer that shows the stills if this script has not started in time. */
    siloPicturesFallback?: number;
  }
}

/** How much of the film has to be in view for it to play. */
const FILM_IN_VIEW = 0.3;

/** Whether enough of `el` is in view now, for the first frame: the observer only answers after it. */
function inViewNow(el: Element) {
  const { top, bottom, height } = el.getBoundingClientRect();
  return height > 0 && (Math.min(bottom, innerHeight) - Math.max(top, 0)) / height >= FILM_IN_VIEW;
}

export function mountPictures({ hold = null }: { hold?: number | null } = {}): { film: Film; cards: Cards } {
  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const root = document.querySelector<HTMLElement>('.film')!;
  const figure = root.querySelector('figure')!;
  let filmInView = inViewNow(figure);
  let film: Film | undefined;
  let cards: Cards | undefined;
  const settle = () => {
    if (!film || !cards) return;
    const visible = document.visibilityState !== 'hidden';
    film.allow(filmInView && visible && !cards.busy);
    cards.turns(visible && !film.running);
  };
  film = mountFilm(root, { reduced, inView: filmInView, hold, onChange: settle });
  cards = mountCards(document.getElementById('cards')!, { still: reduced, onChange: settle });

  new IntersectionObserver(
    (entries) => {
      filmInView = entries[entries.length - 1].intersectionRatio >= FILM_IN_VIEW;
      settle();
    },
    { threshold: FILM_IN_VIEW },
  ).observe(figure);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') cards.rest();
    settle();
  });
  // Both pictures are drawn: they can show.
  window.clearTimeout(window.siloPicturesFallback);
  document.documentElement.classList.remove('pictures-pending');
  settle();
  return { film, cards };
}
