// The one in-memory snapshot that answers every anonymous read. A visitor's
// request never calls GitHub: it reads the snapshot, and when the snapshot is
// over a minute old it starts one background refresh. Only a cold instance
// waits for its first refresh.
//
// A refresh reads, with the installation token:
// - the repository's issue list (all states, oldest first, 100 a page), each
//   page sent with the ETag from the last refresh, so an unchanged page is a 304;
// - the repository-wide comment list: the newest comments in full on the first
//   refresh and every 10 minutes (so deleted comments disappear), and otherwise
//   only comments updated since the previous refresh, less a minute of overlap.
// A refresh builds a new snapshot and swaps it in only when it completed.
//
// Writes patch the snapshot at once. Each patch is kept with its time: after a
// refresh swaps in, the patches made since that refresh began are applied
// again, and older ones are dropped, since GitHub's answer already holds them.
import { GitHubError, nextLink } from './github.mjs';
import { clean, commentIssue, commentOf, kindOf, readBody, reasonOf, verdictOf } from './issues.mjs';

export const SNAPSHOT = {
  maxAge: 60_000,
  staleAfter: 10 * 60_000,
  fullReloadEvery: 10 * 60_000,
  overlap: 60_000,
  retryAfter: 60_000,
  issues: 2000,
  issuePages: 30,
  fullCommentPages: 25,
  sinceCommentPages: 10,
  commentsPerIssue: 100,
  comments: 5000,
  patches: 500,
};

const byAge = (a, b) => (a.createdAt < b.createdAt ? -1 : a.createdAt > b.createdAt ? 1 : a.id - b.id);

// `request(method, path, options)` calls GitHub with the installation token and
// throws a GitHubError when GitHub fails or the installation must wait.
// `detach(fn)` runs fn outside the current request (no request deadline).
// `afterRefresh()` runs, detached, after each successful refresh.
export function createSnapshot({ repo, request, now = Date.now, log = () => {}, detach = (fn) => fn(), afterRefresh = () => {} }) {
  let current = null;
  let pending = null;
  let retryAt = 0;
  let lastError = null;
  let lastFull = 0;
  let fullDue = true;
  let issuePages = new Map();
  let commentPages = new Map();
  let patches = [];

  function item(issue, kind, previous) {
    const { entry, text } = readBody(issue.body);
    return {
      number: issue.number,
      summary: {
        kind,
        entry: kind === 'entry' ? entry : null,
        title: clean(issue.title, { oneLine: true }),
        state: issue.state === 'closed' ? 'closed' : 'open',
        reason: reasonOf(issue),
        verdict: verdictOf(issue),
        up: issue.reactions?.['+1'] ?? 0,
        comments: issue.comments ?? 0,
        author: clean(issue.user?.login ?? 'ghost', { oneLine: true }),
        bot: issue.user?.type === 'Bot',
        createdAt: issue.created_at,
        closedAt: issue.closed_at ?? null,
      },
      body: text,
      url: issue.html_url,
      comments: previous ? new Map(previous.comments) : new Map(),
    };
  }

  // Pages of one listing, each conditional on its last ETag; answers the items and the new page cache.
  async function pages(path, limit, cache) {
    const items = [];
    const fresh = new Map();
    let complete = false;
    for (let page = 1; page <= limit; page += 1) {
      const url = `${path}&per_page=100&page=${page}`;
      const known = cache?.get(url);
      const answer = await request('GET', url, { etag: known?.etag });
      const entry = answer.status === 304 && known
        ? known
        : { etag: answer.headers.get('etag'), items: Array.isArray(answer.data) ? answer.data : [], next: Boolean(nextLink(answer.headers.get('link'))) };
      fresh.set(url, entry);
      items.push(...entry.items);
      if (!entry.next) {
        complete = true;
        break;
      }
    }
    return { items, cache: fresh, complete };
  }

  function trim(issues) {
    for (const entry of issues.values()) {
      if (entry.comments.size <= SNAPSHOT.commentsPerIssue) continue;
      const kept = [...entry.comments.values()].sort(byAge).slice(-SNAPSHOT.commentsPerIssue);
      entry.comments = new Map(kept.map((comment) => [comment.id, comment]));
    }
    let total = 0;
    for (const entry of issues.values()) total += entry.comments.size;
    if (total <= SNAPSHOT.comments) return;
    const all = [];
    for (const entry of issues.values()) for (const comment of entry.comments.values()) all.push([entry, comment]);
    all.sort((a, b) => byAge(a[1], b[1]));
    for (const [entry, comment] of all.slice(0, total - SNAPSHOT.comments)) entry.comments.delete(comment.id);
  }

  async function build() {
    const started = now();
    const listing = await pages(`/repos/${repo}/issues?state=all&sort=created&direction=asc`, SNAPSHOT.issuePages, issuePages);
    if (!listing.complete) log({ snapshot: 'issue_pages_limit' });

    const full = fullDue || !current || started - lastFull >= SNAPSHOT.fullReloadEvery;
    const issues = new Map();
    for (const issue of listing.items) {
      if (issues.size >= SNAPSHOT.issues) break;
      const kind = kindOf(issue);
      if (!kind || issues.has(issue.number)) continue;
      issues.set(issue.number, item(issue, kind, full ? null : current?.issues.get(issue.number)));
    }

    let comments;
    let incompleteSince = false;
    if (full) {
      comments = await pages(`/repos/${repo}/issues/comments?sort=created&direction=desc`, SNAPSHOT.fullCommentPages, commentPages);
    } else {
      const since = new Date(current.startedAt - SNAPSHOT.overlap).toISOString().replace(/\.\d{3}Z$/, 'Z');
      comments = await pages(`/repos/${repo}/issues/comments?sort=updated&direction=asc&since=${since}`, SNAPSHOT.sinceCommentPages, null);
      incompleteSince = !comments.complete;
    }
    for (const comment of comments.items) {
      const entry = issues.get(commentIssue(comment));
      if (entry) entry.comments.set(comment.id, commentOf(comment));
    }
    trim(issues);

    issuePages = listing.cache;
    if (full) {
      commentPages = comments.cache;
      lastFull = started;
    }
    fullDue = incompleteSince;
    return { startedAt: started, at: now(), issues, version: 0, stateAnswer: null };
  }

  function swapIn(snapshot) {
    patches = patches.filter((patch) => patch.at >= snapshot.startedAt);
    for (const patch of patches) patch.apply(snapshot);
    current = snapshot;
  }

  function refresh() {
    pending ??= detach(() => build())
      .then((snapshot) => {
        swapIn(snapshot);
        retryAt = 0;
        lastError = null;
        try {
          detach(afterRefresh);
        } catch {
          // The follow-up work is best effort and never fails the refresh.
        }
        return snapshot;
      }, (error) => {
        const wait = error instanceof GitHubError && error.retryAfter ? error.retryAfter * 1000 : 0;
        retryAt = now() + Math.max(SNAPSHOT.retryAfter, wait);
        lastError = error;
        log({ snapshot: 'refresh_failed', ...(error instanceof GitHubError ? { github: error.status } : { failure: error?.name || 'Error' }) });
        throw error;
      })
      .finally(() => { pending = null; });
    return pending;
  }

  // The snapshot to answer from and whether it is stale; throws when there is none.
  async function read() {
    const at = now();
    if (!current) {
      if (at < retryAt) throw lastError instanceof GitHubError ? lastError : new GitHubError(502);
      await refresh();
    } else if (at - current.startedAt >= SNAPSHOT.maxAge && at >= retryAt && !pending) {
      refresh().catch(() => {});
    }
    return { snapshot: current, stale: now() - current.at > SNAPSHOT.staleAfter };
  }

  function stateAnswer(snapshot) {
    if (!snapshot.stateAnswer || snapshot.stateAnswer.version !== snapshot.version) {
      const issues = {};
      for (const [number, entry] of snapshot.issues) issues[number] = entry.summary;
      snapshot.stateAnswer = { version: snapshot.version, body: { repo, fetchedAt: new Date(snapshot.at).toISOString(), issues } };
    }
    return snapshot.stateAnswer.body;
  }

  function issueAnswer(snapshot, number) {
    const entry = snapshot.issues.get(number);
    if (!entry) return null;
    const comments = [...entry.comments.values()].sort(byAge);
    return {
      number,
      title: entry.summary.title,
      body: entry.body,
      url: entry.url,
      up: entry.summary.up,
      more: entry.summary.comments > comments.length,
      comments,
    };
  }

  // Records a patch and applies it to the current snapshot. `apply(snapshot)` must be safe to run
  // again on a newer snapshot: counts move only from the value they had when the write happened.
  function record(apply) {
    patches.push({ at: now(), apply });
    if (patches.length > SNAPSHOT.patches) patches.shift();
    if (current) apply(current);
  }

  function onEntry(number, change) {
    return (snapshot) => {
      const entry = snapshot.issues.get(number);
      if (!entry) return;
      change(entry);
      snapshot.version += 1;
    };
  }

  // A count moves from `from` to `to` only when the snapshot still shows `from`.
  function count(number, field, delta) {
    const entry = current?.issues.get(number);
    if (!entry) return;
    const from = entry.summary[field];
    const to = Math.max(0, from + delta);
    record(onEntry(number, (target) => { if (target.summary[field] === from) target.summary[field] = to; }));
  }

  const patch = {
    comment(number, comment) {
      const shown = commentOf(comment);
      count(number, 'comments', 1);
      record(onEntry(number, (entry) => {
        if (entry.comments.has(shown.id)) return;
        entry.comments.set(shown.id, shown);
        trim(new Map([[number, entry]]));
      }));
    },
    up(number, delta) {
      count(number, 'up', delta);
    },
    fields(number, fields) {
      record(onEntry(number, (entry) => Object.assign(entry.summary, fields)));
    },
    issue(issue, kind) {
      record((snapshot) => {
        if (snapshot.issues.has(issue.number) || snapshot.issues.size >= SNAPSHOT.issues) return;
        snapshot.issues.set(issue.number, item(issue, kind, null));
        snapshot.version += 1;
      });
    },
  };

  // Settles once no refresh is running (for tests and graceful shutdown).
  const idle = () => (pending ? pending.then(() => {}, () => {}) : Promise.resolve());

  return { read, stateAnswer, issueAnswer, patch, idle };
}
