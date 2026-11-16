import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import { buildPlan } from './plan';
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
    <div className="ws-plain">
      <div className="ws-top">
        <div>
          <h1>Workshop</h1>
          <p>Silo is being rebuilt from scratch. This is the plan, in build order.</p>
        </div>
      </div>
      {plan?.stages.map((group) => (
        <section key={group.stage.id} aria-label={group.stage.title}>
          <div className="ws-stage"><span className="ws-num">{group.number}</span><h2>{group.stage.title}</h2><span /></div>
          <ul>
            {group.rows.map((row) => <li key={row.key} className="ws-plain-row">{row.name}</li>)}
          </ul>
        </section>
      ))}
      <div className="ws-end" />
    </div>
  );
}
