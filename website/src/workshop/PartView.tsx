// An opened part: its title, line, one dot per feature and the counts on the left, All's rows on the right: its
// features, each with the decisions it needs folded under it, then its decisions that none of its features needs. The
// same two columns hold Product's decisions, a film and the suggestions.
import type { ReactNode } from 'react';
import { AllList } from './AllList';
import { filmSections, partFilm, partSections, productSections } from './all';
import type { Graph, Section } from './all';
import { Entry, pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { FilmControl, FilmFrame } from './Films';
import { countDecisions, countRows } from './plan';
import type { Counts, DecisionCounts, FilmItem, PartGroup, Plan, Row } from './plan';
import type { RelationStore } from './relation';

export interface BlockProps {
  num?: string;
  /** `num` says what kind of place this is ("Film"), not a number. */
  kind?: boolean;
  title: string;
  line?: string;
  onBack: () => void;
  children?: ReactNode;
  list: ReactNode;
  /** Quiet lines under the counts: the API unavailable, sign-in failed. */
  notes?: ReactNode;
}

/** The two columns every opened place uses. */
export function Block({ num, kind = false, title, line, onBack, children, list, notes }: BlockProps) {
  return (
    <>
      <div className="ws-block">
        <button type="button" className="ws-back" onClick={onBack}>
          <span className="wa-wide">← Full map</span><span className="wa-narrow">← All parts</span>
        </button>
        <span className={kind ? 'ws-num wa-kind' : 'ws-num'}>{num ?? ''}</span>
        <h2 id="ws-part-title" tabIndex={-1}>{title}</h2>
        {line ? <p className="ws-aline">{line}</p> : null}
        {children}
        {notes ? <div className="ws-notes wide">{notes}</div> : null}
      </div>
      <div className="ws-list">{list}{notes ? <div className="ws-notes narrow">{notes}</div> : null}</div>
    </>
  );
}

export function Rows({ rows, ctx }: { rows: Row[]; ctx: EntryContext }) {
  return <ul className="ws-rows">{rows.map((row) => <Entry key={row.key} row={row} open={ctx.open.has(row.key)} ctx={ctx} />)}</ul>;
}

/** What a place's list needs beyond its rows: the same as All's. */
export interface PlaceList {
  plan: Plan;
  graph: Graph;
  ctx: EntryContext;
  store: RelationStore;
  folds: ReadonlyMap<string, boolean>;
  onFold: (key: string, open: boolean) => void;
  more: ReadonlySet<string>;
  onMore: (key: string) => void;
  waiting: boolean;
}

function PlaceRows({ sections, list, inPart = null, flat = false }: { sections: Section[]; list: PlaceList; inPart?: string | null; flat?: boolean }) {
  return (
    <AllList
      sections={sections.filter((section) => section.items.length)} graph={list.graph} plan={list.plan} ctx={list.ctx} store={list.store} inPart={inPart} flat={flat}
      folds={list.folds} onFold={list.onFold} more={list.more} onMore={list.onMore} waiting={list.waiting}
    />
  );
}

/** Each count stays on one line; lines break after a separator. */
const one = (n: number, words: string) => `${n} ${words}`.replace(/ /g, '\u00a0');
const joined = (items: ReactNode[]) => items.flatMap((item, index) => (index ? ['\u00a0· ', item] : [item]));

/** The counts of a part's features. */
function tally(counts: Counts): ReactNode[] {
  const { built, building, decided, undecided, dropped } = counts;
  const items: ReactNode[] = [];
  if (built) items.push(one(built, 'built'));
  if (building) items.push(one(building, 'being built'));
  if (decided) items.push(one(decided, 'decided'));
  if (undecided) items.push(one(undecided, 'not decided'));
  if (dropped) items.push(one(dropped, 'dropped'));
  return joined(items);
}

/** The counts of a part's decisions, on a line of their own after "Decisions:". */
function decisionTally(counts: DecisionCounts): ReactNode[] {
  const items: ReactNode[] = [];
  if (counts.decided) items.push(one(counts.decided, 'decided'));
  if (counts.undecided) items.push(one(counts.undecided, 'not decided'));
  if (counts.replaced) items.push(one(counts.replaced, 'replaced'));
  if (counts.dropped) items.push(one(counts.dropped, 'dropped'));
  return joined(items);
}

/** The tallies of a list of rows: its features', then its decisions'. */
function Tallies({ rows }: { rows: Row[] }) {
  const features = tally(countRows(rows.filter((row) => row.of === 'feature')));
  const decisions = decisionTally(countDecisions(rows.filter((row) => row.of === 'decision')));
  return (
    <>
      {features.length ? <p className="ws-tally">{features}</p> : null}
      {decisions.length ? <p className="ws-tally">Decisions: {decisions}</p> : null}
    </>
  );
}

export function PartPlace({ group, list, onBack, notes, onFilm }: { group: PartGroup; list: PlaceList; onBack: () => void; notes?: ReactNode; onFilm: (film: string) => void }) {
  const film = partFilm(list.plan, group.part.id);
  const sections = partSections(list.graph, group.rows);
  return (
    <Block
      num={String(group.number).padStart(2, '0')} title={group.part.title} line={group.part.line} onBack={onBack} notes={notes}
      list={<PlaceRows sections={sections} list={list} inPart={group.part.id} />}
    >
      <div className="ws-pips" aria-hidden="true">
        {group.rows.filter((row) => row.of === 'feature' && row.status !== 'dropped').map((row) => <i className={pipClass(row)} key={row.key} />)}
      </div>
      <Tallies rows={group.rows} />
      {film ? <FilmControl film={film} pressed={false} onOpen={onFilm} /> : null}
    </Block>
  );
}

/** Product: the decisions that are not about one part, from plan/decisions/. */
export function ProductPlace({ rows, list, onBack, notes }: { rows: Row[]; list: PlaceList; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      title="Product" line="The decisions that are not about one part." onBack={onBack} notes={notes}
      list={<PlaceRows sections={productSections(rows)} list={list} flat />}
    >
      <Tallies rows={rows} />
    </Block>
  );
}

/** A film's place: the film, then the entries it shows, each on its own line. */
export function FilmPlace({ film, rows, list, onBack, notes }: { film: FilmItem; rows: Row[]; list: PlaceList; onBack: () => void; notes?: ReactNode }) {
  const sections = filmSections(list.graph, rows);
  return (
    <Block
      num="Film" kind title={film.title} line={film.line} onBack={onBack} notes={notes}
      list={(
        <>
          {film.playable ? <div className="wa-player"><FilmFrame film={film} /></div> : null}
          <PlaceRows sections={sections} list={list} flat />
        </>
      )}
    />
  );
}
