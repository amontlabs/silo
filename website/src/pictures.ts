// The home page has two pictures: the flow and the cards. This starts both and says which may move.
//
// Only one thing moves on its own at a time. The flow plays while at least 30% of it is in view, and the cards
// take turns only while the flow is not playing. A card the visitor points at or presses holds the flow until
// they leave it or it ends. A hidden tab holds everything. With reduced motion nothing moves until the visitor
// presses Play or a card.

import { mountCards, type Cards } from './cards';
import { mountFlow, type Flow } from './flow';

declare global {
  interface Window {
    /** The head script's timer that shows the stills if this script has not started in time. */
    siloPicturesFallback?: number;
  }
}

/** How much of the flow has to be in view for it to play. */
const FLOW_IN_VIEW = 0.3;

export function mountPictures(): { flow: Flow; cards: Cards } {
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const flowRoot = document.getElementById('flow')!;
  let flowInView = false;
  let flow: Flow | undefined;
  let cards: Cards | undefined;
  const settle = () => {
    if (!flow || !cards) return;
    const visible = document.visibilityState !== 'hidden';
    flow.allow(flowInView && visible && !cards.busy);
    cards.turns(visible && !flow.running);
  };
  flow = mountFlow(flowRoot, { still, onChange: settle });
  cards = mountCards(document.getElementById('cards')!, { still, onChange: settle });

  new IntersectionObserver(
    (entries) => {
      flowInView = entries[entries.length - 1].intersectionRatio >= FLOW_IN_VIEW;
      settle();
    },
    { threshold: FLOW_IN_VIEW },
  ).observe(flowRoot.querySelector('.stage')!);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') cards.rest();
    settle();
  });
  // Both pictures are drawn: they can show.
  window.clearTimeout(window.siloPicturesFallback);
  document.documentElement.classList.remove('pictures-pending');
  settle();
  return { flow, cards };
}
