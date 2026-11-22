// What the cards' tests need and jsdom lacks: animations that only say what runs.

interface Run {
  target: Element;
  frames: Keyframe[];
  options: KeyframeAnimationOptions;
  cancelled: boolean;
}

/** Records every animation started from now on, and reads from them what moves. */
export function recordAnimations() {
  let runs: Run[] = [];
  Element.prototype.animate = function (this: Element, frames: Keyframe[], options: KeyframeAnimationOptions) {
    const run: Run = { target: this, frames, options, cancelled: false };
    runs.push(run);
    return { effect: { target: this }, cancel: () => (run.cancelled = true) };
  } as unknown as Element['animate'];
  const live = () => (runs = runs.filter((run) => !run.cancelled));
  const named = (target: Element) => target.closest<HTMLElement>('.fc-item')!.dataset.picture!;
  return {
    /** Every animation that has not been cancelled. */
    live,
    /** The cards whose line fills: those that play. */
    playing: () => live().filter(({ target, frames }) => target.matches('.fc-fill') && frames[0].transform === 'scaleX(0)').map(({ target }) => named(target)),
    /** The cards whose picture moves. A copy that fades is not the picture. */
    moving: () => [...new Set(live().filter(({ target }) => target.hasAttribute('data-part')).map(({ target }) => named(target)))],
  };
}
