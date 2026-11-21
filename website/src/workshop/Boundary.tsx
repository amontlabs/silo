import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import { allRows, buildPlan, splitRows, statusLine } from './plan';
import type { BuildData, Row } from './plan';

/** If the Workshop fails to render, the plan stays readable from the data built into the page: each part's features and then its decisions, with their state, then the decisions of Product. */
export class Boundary extends Component<{ data: BuildData; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error('Workshop failed to render', error, info.componentStack);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return <PlainPlan data={this.props.data} />;
  }
}

/** What the list says of a row: its status in the page's words, a decision's own. */
function plainState(row: Row): string {
  switch (row.status) {
    case 'undecided': return 'not decided';
    case 'building': return 'being built';
    default: return row.status;
  }
}

function PlainList({ rows, all }: { rows: Row[]; all: Map<string, Row> }) {
  return (
    <ul>
      {rows.map((row) => {
        const by = row.replacedBy === null ? undefined : all.get(row.replacedBy);
        return <li key={row.key}>{row.name} <small>({row.of === 'decision' ? 'decision' : 'feature'}, {plainState(row)})</small>{by ? <small>{` Replaced by ${by.name}`}</small> : null}</li>;
      })}
    </ul>
  );
}

/** A part's features under "Features", then its decisions under "Decisions"; a group with no row shows nothing. Product's decisions are one list. */
function PlainRows({ title, rows, all, headed = true }: { title: string; rows: Row[]; all: Map<string, Row>; headed?: boolean }) {
  const { features, decisions } = splitRows(rows);
  return (
    <section aria-label={title}>
      <h2>{title}</h2>
      {headed ? ([['Features', features], ['Decisions', decisions]] as const).filter(([, group]) => group.length).map(([name, group]) => (
        <div key={name}>
          <h3>{name}</h3>
          <PlainList rows={group} all={all} />
        </div>
      )) : <PlainList rows={rows} all={all} />}
    </section>
  );
}

function PlainPlan({ data }: { data: BuildData }) {
  let plan: ReturnType<typeof buildPlan> | null = null;
  try {
    plan = buildPlan(data, null);
  } catch {
    plan = null;
  }
  const all = new Map(plan ? allRows(plan).map((row) => [row.key, row]) : []);
  return (
    <div className="ws ws-plain">
      <div className="ws-top">
        <div className="ws-head">
          <h1>Workshop</h1>
          <p className="ws-status">{plan ? statusLine(plan.parts) : ''}</p>
        </div>
      </div>
      {plan?.parts.map((group) => <PlainRows key={group.part.id} title={group.part.title} rows={group.rows} all={all} />)}
      {plan?.product.length ? <PlainRows title="Product" rows={plan.product} all={all} headed={false} /> : null}
    </div>
  );
}
