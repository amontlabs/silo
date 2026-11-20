// Suggest a feature: the form, or a way to sign in, then what people have suggested, by votes.
import type { ReactNode } from 'react';
import { Block, Rows } from './PartView';
import type { EntryContext } from './Entry';

export function SuggestPlace({ ctx, onBack, notes }: { ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  const { w } = ctx;
  const { suggestDraft, setSuggestDraft } = w;
  let form: ReactNode = null;
  if (w.me === undefined) {
    // Who is signed in is not known yet: the form's room is held by the form's own boxes, drawn as placeholders.
    form = (
      <div aria-busy="true">
        <div className="ws-form ws-ph-shape" aria-hidden="true" inert>
          <input tabIndex={-1} disabled />
          <textarea tabIndex={-1} rows={2} disabled />
          <div><span className="ws-btn go">Suggest</span></div>
        </div>
      </div>
    );
  } else if (w.me) {
    // Shown whenever the person is, and its draft kept; it sends only once a state is shown.
    form = (
      <form className="ws-form ws-late-p" onSubmit={(event) => void w.sendSuggestion(event)}>
        <input aria-label="Title" placeholder="Name it" maxLength={120} required value={suggestDraft.title} onChange={(event) => setSuggestDraft({ ...suggestDraft, title: event.target.value })} />
        <textarea aria-label="Why" placeholder="What should it do, and why?" maxLength={5000} rows={2} value={suggestDraft.text} onChange={(event) => setSuggestDraft({ ...suggestDraft, text: event.target.value })} />
        <div><button type="submit" className="ws-btn go" disabled={!suggestDraft.title.trim() || !w.liveOk}>Suggest</button></div>
      </form>
    );
  } else if (w.me === null) {
    form = <div className="ws-form ws-late-p"><a className="ws-signin" href={ctx.signIn}>Sign in to suggest</a></div>;
  }
  const people = w.plan.suggestions.length + w.suggestionsSending.length;
  return (
    <Block
      num="+" title="Suggest a feature" line="Something Silo should do that is not here yet." onBack={onBack} notes={notes}
      list={(
        <>
          {form}
          <section aria-label="Suggested by people" aria-busy={w.waiting || undefined}>
            <h3 className="ws-grp">Suggested by people</h3>
            {w.waiting ? (
              <ul className="ws-rows">
                <li className="ws-item ws-ph-row" aria-hidden="true">
                  <div className="ws-line">
                    <div className="ws-row"><i className="ws-pip" /><span className="ws-name"><i className="ws-ph" /></span><b className="ws-word" /></div>
                    <span className="ws-side"><span className="ws-votes"><i className="ws-ph ws-ph-vote" /></span><span className="ws-talks" /></span>
                  </div>
                </li>
              </ul>
            ) : null}
            {people || w.waiting ? null : <p className="ws-empty">{w.liveOk ? 'Nothing yet.' : 'Suggestions are offline.'}</p>}
            <Rows rows={w.plan.suggestions} ctx={ctx} />
            {w.suggestionsSending.length ? (
              <ul className="ws-rows">
                {w.suggestionsSending.map((sending) => (
                  <li className="ws-item quiet ws-pending" key={sending.id} aria-busy="true">
                    <div className="ws-line">
                      <div className="ws-row">
                        <i className="ws-pip undecided" aria-hidden="true" />
                        <span className="ws-name">{sending.title}</span>
                        <b className="ws-word">Sending…</b>
                      </div>
                      <span className="ws-side"><span className="ws-by">{sending.author}</span><span className="ws-votes" /><span className="ws-talks" /></span>
                    </div>
                  </li>
                ))}
              </ul>
            ) : null}
          </section>
        </>
      )}
    />
  );
}
