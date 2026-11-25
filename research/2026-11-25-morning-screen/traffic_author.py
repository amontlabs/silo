"""Who opened a connection, from model traffic alone: #399's rule for connections, and the fix.

#399 names a connection's conversation when one open tool call's text holds the host (a substring
test), else when only one conversation was open, else by the program's own hosts. On this folder's
night that names a wrong conversation for 31 of the 55 connections the probe gives a run (checks/connections_wrong.py):

- `github.com` is a substring of `api.github.com`, so Codex's start-up connection to github.com went
  to the Claude Code run whose `curl` named api.github.com.
- A program that never reaches its model has no conversation. Codex refusing Silo's certificate
  (#403) opened 30 connections while Claude Code's daemon run was the only conversation open, and
  "only one" gave them to it.

#399's own score could not see either: it scores against the probe's conversation, and the run that
opened them has none, so they counted as "unattributed by the probe".

- A tool call that names a host another program reaches at its start is not enough: Codex reached
  api.github.com at start while Claude Code's `curl` named it, and the two cannot be told apart
  from traffic.

The fix: a host is named only on a host boundary; "only one conversation was open" never names a
connection; a host that another program reaches at its start names nobody. A connection then names
a conversation only when one conversation's tool call names its host and no other program reaches
it at start, or when it is the program's own host and one conversation of that program was running.
"""
import re

OWN_HOSTS = {"codex": {"chatgpt.com", "ab.chatgpt.com"}, "claude": {"statsig.anthropic.com", "sentry.io"}}
# hosts a program reaches at its start, before any work (#371 finding 4, #403)
START_HOSTS = {"codex": {"github.com", "api.github.com", "chatgpt.com", "ab.chatgpt.com"}, "claude": {"api.anthropic.com"}}


def names_host(text, host):
    return re.search(r"(?<![\w.-])" + re.escape(host) + r"(?![\w-]|\.[\w-])", text or "") is not None


def connections(view, calls, progs, conv_span, ts_of, night_end):
    """{connection fact id: (conversation or None, how, tool call id or None)}"""
    out = {}
    for l in view.facts("connection"):
        at = ts_of(l.get("started") or l["at"])
        cands = [t for t in calls if t.computer == l["computer"] and t.start <= at <= (t.end or night_end)]
        named = {(t.conversation, t.id) for t in cands if names_host(t.text, l["host"])}
        convs = {c for c, _ in named}
        other_start = [p for p, hs in START_HOSTS.items() if l["host"] in hs and not any(progs.get(c) == p for c in convs)]
        if len(convs) == 1 and other_start:
            out[l["id"]] = (None, f"named, and a start host of {other_start[0]}", None)
            continue
        if len(convs) == 1:
            tcs = [i for _, i in named]
            out[l["id"]] = (convs.pop(), "named", tcs[0] if len(tcs) == 1 else None)
            continue
        if convs:
            out[l["id"]] = (None, "named, several conversations", None)
            continue
        prog = next((p for p, hs in OWN_HOSTS.items() if l["host"] in hs), None)
        if prog:
            live = {c for c, (s0, e) in conv_span.items() if progs[c] == prog and c.split(":")[0] == l["computer"] and s0 - 30 <= at <= e + 5}
            out[l["id"]] = (live.pop() if len(live) == 1 else None, "own host", None)
            continue
        out[l["id"]] = (None, "none", None)
    return out
