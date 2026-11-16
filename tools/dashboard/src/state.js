// What an item is, from its replies alone. The page, the CLI and the tests all
// ask these functions, so "For you" has one definition.

export const KIND_ORDER = { decide: 0, look: 1, shipped: 2, note: 3 };

function lastAgentReplyId(item) {
  let last = 0;
  for (const reply of item.replies) if (reply.author === 'agent' && reply.id > last) last = reply.id;
  return last;
}

export function lastReplyId(item) {
  let last = 0;
  for (const reply of item.replies) if (reply.id > last) last = reply.id;
  return last;
}

// The value stored for a shipped item or a note the owner marked seen: the
// newest reply of the item at that moment, so a later agent reply makes it
// wait again.
export function seenValue(item, now) {
  return JSON.stringify({ at: now, after: lastReplyId(item) });
}

function parseSeen(value) {
  if (!value) return null;
  try {
    const parsed = JSON.parse(value);
    return Number.isInteger(parsed.after) ? parsed : null;
  } catch {
    return null;
  }
}

// 'closed' | 'waiting' | 'answered'. A note starts with the owner's reply, so
// it waits only after an agent replies to it.
export function itemState(item, seen) {
  if (item.status === 'closed') return 'closed';
  const agent = lastAgentReplyId(item);
  if (item.replies.some((reply) => reply.author === 'owner' && reply.id > agent)) return 'answered';
  if (item.kind === 'shipped' || item.kind === 'note') {
    const mark = parseSeen(seen);
    if (mark && mark.after >= agent) return 'answered';
  }
  return 'waiting';
}

export function lastActivity(item) {
  let latest = item.closed_at || item.created_at;
  for (const reply of item.replies) if (reply.created_at > latest) latest = reply.created_at;
  return latest;
}

export function forYouOrder(a, b) {
  return KIND_ORDER[a.kind] - KIND_ORDER[b.kind] || b.id - a.id;
}
