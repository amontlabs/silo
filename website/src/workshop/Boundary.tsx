import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import { buildPlan, statusLine } from './plan';
import type { BuildData, Row } from './plan';

/** If the Workshop fails to render, the plan stays readable from the data built into the page: each part's decisions and features with their state, then the whole product's decisions. */
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
  if (row.of === 'decision') return row.status === 'planned' ? 'decided' : row.status === 'dropped' ? 'dropped' : 'not decided';
  return row.status === 'undecided' ? 'not decided' : row.status;
}

function PlainRows({ title, rows }: { title: string; rows: Row[] }) {
  return (
    <section aria-label={title}>
      <h2>{title}</h2>
      <ul>
        {rows.map((row) => <li key={row.key}>{row.name} <small>({row.of === 'decision' ? 'decision' : 'feature'}, {plainState(row)})</small></li>)}
      </ul>
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
  return (
    <div className="ws ws-plain">
      <div className="ws-top">
        <div className="ws-head">
          <h1>Workshop</h1>
          <p className="ws-status">{plan ? statusLine(plan.parts) : ''}</p>
        </div>
      </div>
      {plan?.parts.map((group) => <PlainRows key={group.part.id} title={group.part.title} rows={group.rows} />)}
      {plan?.product.length ? <PlainRows title="The whole product" rows={plan.product} /> : null}
    </div>
  );
}
