// What happened lately across the entries: the newest comments and history lines, each linking to its entry.
import type { MouseEvent, ReactNode } from 'react';
import { Block } from './AreaView';
import { pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { changeLine, relativeTime } from './plan';
import type { LatestItem, Row } from './plan';
import { formatHash, placeOf } from './routes';

/** How many lines hold Latest's place while it loads. */
export const LATEST_GHOSTS = 10;

function what(item: LatestItem): string {
  if (item.change) return changeLine(item.change);
  return `${item.author}: ${item.text ?? ''}`;
}

/** A plain click opens the entry here; a click with a modifier key is left to the browser (a new tab, a window). */
function opener(ctx: EntryContext, row: Row) {
  return (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    ctx.onToggle(row);
  };
}

/** A line that holds the place of one still to come. */
export const ghost = (index: number) => (
  <li className="ws-item ws-latest-ghost" key={`ghost-${index}`} aria-hidden="true">
    <div className="ws-line">
      <div className="ws-row">
        <i className="ws-pip" />
        <span className="ws-name"><span className="ws-latest-name">{'\u00a0'}</span><small className="ws-sub">{'\u00a0'}</small></span>
        <b className="ws-word" />
      </div>
    </div>
  </li>
);

export function LatestPlace({ ctx, onBack, notes }: { ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  const { w } = ctx;
  const items = w.plan.latest;
  const loading = w.waiting;
  let empty: ReactNode = null;
  if (!items.length && !loading) empty = <p className="ws-empty">{w.liveOk ? 'Nothing yet.' : 'Latest is offline.'}</p>;
  return (
    <Block
      title="Latest" onBack={onBack} notes={notes} loading={loading}
      list={(
        <>
          {empty}
          <ul className="ws-rows ws-latest" aria-busy={loading || undefined}>
            {loading ? Array.from({ length: LATEST_GHOSTS }, (_, index) => ghost(index)) : null}
            {items.map((item) => (
              <li className="ws-item ws-late-s" key={item.key}>
                <div className="ws-line">
                  <a className="ws-row" href={formatHash(placeOf(item.row), item.row.key)} onClick={opener(ctx, item.row)}>
                    <i className={pipClass(item.row)} aria-hidden="true" />
                    <span className="ws-name"><span className="ws-latest-name">{item.row.name}</span><small className={item.change ? 'ws-sub ws-sub-h' : 'ws-sub'}>{what(item)}</small></span>
                    <b className="ws-word">{relativeTime(item.createdAt)}</b>
                  </a>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
    />
  );
}
