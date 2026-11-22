// The home page has two pictures: the film and the cards. This starts both and says which may move.
//
// One thing moves at a time. The film plays while at least 30% of it is in view, and the cards take turns only
// while nothing in the film moves: as it starts or jumps, the cards stop at once. A card the visitor points at or
// presses holds the film until they leave it, it ends or they take the film with Play or a jump. A hidden tab
// holds everything. With reduced motion the cards stay at their end, the film moves only once the visitor presses
// Play, and a jump is instant. When reduced motion turns on everything stops, and when it turns off nothing
// starts by itself.

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
  const motion = matchMedia('(prefers-reduced-motion: reduce)');
  const root = document.querySelector<HTMLElement>('.film')!;
  const figure = root.querySelector('figure')!;
  let filmInView = inViewNow(figure);
  let film: Film | undefined;
  let cards: Cards | undefined;
  const settle = () => {
    if (!film || !cards) return;
    const visible = document.visibilityState !== 'hidden';
    film.allow(filmInView && visible && !cards.busy);
    if (cards.busy) film.land();
    cards.turns(visible && !film.moving);
  };
  const take = () => {
    cards?.giveWay();
    settle();
  };
  film = mountFilm(root, { reduced: motion.matches, inView: filmInView, hold, onTake: take, onChange: settle });
  cards = mountCards(document.getElementById('cards')!, { still: motion.matches, onChange: settle });
  motion.addEventListener('change', () => {
    // The film first: told of the cards alone, it would play on as they let go of it.
    film.reduce(motion.matches);
    cards.still(motion.matches);
    settle();
  });

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
  // The film is drawn: it can show. The cards show from their markup.
  window.clearTimeout(window.siloPicturesFallback);
  document.documentElement.classList.remove('pictures-pending');
  settle();
  return { film, cards };
}
