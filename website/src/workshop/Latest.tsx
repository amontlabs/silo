// What happened lately across the entries: the newest comments and history lines, each linking to its entry.
import type { ReactNode } from 'react';
import { Block } from './AreaView';
import { pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { changeWords, relativeTime } from './plan';
import type { LatestItem } from './plan';
import { formatHash, placeOf } from './routes';

function what(item: LatestItem): string {
  if (item.change) return changeWords(item.change).join(', ');
  return `${item.author}: ${item.text ?? ''}`;
}

export function LatestPlace({ ctx, onBack, notes }: { ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  const { w } = ctx;
  const items = w.plan.latest;
  let empty: ReactNode = null;
  if (!items.length && w.live.status !== 'loading') empty = <p className="ws-empty">{w.liveOk ? 'Nothing yet.' : 'Latest is offline.'}</p>;
  return (
    <Block
      title="Latest" onBack={onBack} notes={notes}
      list={(
        <>
          {empty}
          <ul className="ws-rows ws-latest">
            {items.map((item) => (
              <li className="ws-item" key={item.key}>
                <div className="ws-line">
                  <a
                    className="ws-row" href={formatHash(placeOf(item.row), item.row.key)}
                    onClick={(event) => { event.preventDefault(); ctx.onToggle(item.row); }}
                  >
                    <i className={pipClass(item.row)} aria-hidden="true" />
                    <span className="ws-name">{item.row.name}<small className={item.change ? 'ws-sub ws-sub-h' : 'ws-sub'}>{what(item)}</small></span>
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
