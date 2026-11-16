import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import { buildPlan, statusLine } from './plan';
import type { BuildData } from './plan';

/** If the Workshop fails to render, the plan stays readable from the data built into the page. */
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
          <p className="ws-status">{plan ? statusLine(plan.areas) : ''}</p>
        </div>
      </div>
      {plan?.areas.map((group) => (
        <section key={group.area.id} aria-label={group.area.title}>
          <h2>{group.area.title}</h2>
          <ul>
            {group.rows.map((row) => <li key={row.key}>{row.name}</li>)}
          </ul>
        </section>
      ))}
    </div>
  );
}
