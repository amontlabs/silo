// What a reader without JavaScript gets under the plan: every part with its decisions and features, and the
// decisions of the whole product, as a plain list (id, name, state), after the line that says what needs JavaScript.
// It goes inside the page's <noscript>, so a browser that runs the script never draws it.
import { stateText } from './Entry';
import { allRows, buildPlan } from './plan';
import type { BuildData, Row } from './plan';

export const NOSCRIPT_LINE = 'Reading an entry\'s notes, seeing the suggestions, suggesting a feature, voting and commenting on suggestions, and choosing a color theme need JavaScript.';

function Rows({ rows, all }: { rows: Row[]; all: Map<string, Row> }) {
  return (
    <ul className="ws-noscript-rows">
      {rows.map((row) => {
        const by = row.replacedBy === null ? undefined : all.get(row.replacedBy);
        return (
          <li key={row.key}>
            <code>{row.key.replace(/^entry-/, '')}</code>{` ${row.name} `}<span className="ws-noscript-state">{stateText(row)}</span>
            {by ? <>{' Replaced by '}<code>{by.key.replace(/^entry-/, '')}</code>{` ${by.name}`}</> : null}
          </li>
        );
      })}
    </ul>
  );
}

export function NoScript({ data }: { data: BuildData }) {
  const plan = buildPlan(data, null);
  const all = new Map(allRows(plan).map((row) => [row.key, row]));
  return (
    <div className="ws-noscript">
      <p className="ws-quiet">{NOSCRIPT_LINE}</p>
      {plan.parts.map((group) => (
        <section key={group.part.id} data-part={group.part.id}>
          <h2>{group.part.title}</h2>
          {group.part.line ? <p>{group.part.line}</p> : null}
          {group.rows.length ? <Rows rows={group.rows} all={all} /> : null}
        </section>
      ))}
      {plan.product.length ? (
        <section data-part="product">
          <h2>The whole product</h2>
          <Rows rows={plan.product} all={all} />
        </section>
      ) : null}
    </div>
  );
}
