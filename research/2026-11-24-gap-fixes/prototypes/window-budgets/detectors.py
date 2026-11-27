"""Three alerts on what model traffic shows going wrong, run over a trace of model calls.

A trace is JSON lines, one per model call the boundary saw, in the shape fake_anthropic_usage.py
writes (and tracegen.py makes up): t, computer, conversation (or request.metadata), main,
request {model, cache_ttls, fallbacks}, response {model, usage}, headers (the unified rate-limit
headers of a subscription, when there are any).

  1. model   : the model that answered is not the model asked (1a); the program asks another model
               in the middle of a conversation (1b); a new conversation starts on another model
               than the last one did (1c).
  2. cache   : the cache lifetime the program asks for changes (2a); a prefix that was cached and
               still alive is not read (2b); the uncached share of input over the last hour passes
               a level (2c).
  3. pace    : the week's reported figure is ahead of an even pace through the week by a margin,
               while this computer spends (3a, on the provider's figure); the computer's own
               estimated share rises faster than a pace (3b, on an estimate: a precaution).

Every trigger except 3b reads only what the boundary saw, so it can be a rule. Each alert fires
on a change and not on every call: once per new state, as `alert-only-on-a-change` says.

Run: python3 detectors.py <trace.jsonl> [--expected-model claude-opus-5-5]
"""
import json, re, sys
from collections import defaultdict, deque

HOUR, DAY = 3600.0, 86400.0


def norm(model):
    """A model name without a date snapshot or a platform suffix, for comparing asked and answered."""
    m = (model or "").lower().split("@")[0]
    m = re.sub(r"-\d{8}$", "", m)
    return re.sub(r"\[.*\]$", "", m)


def conv_key(r):
    if r.get("conversation"):
        return r["conversation"]
    md = (r.get("request") or {}).get("metadata") or {}
    uid = md.get("user_id")
    if isinstance(uid, str) and uid.startswith("{"):
        try:
            return json.loads(uid).get("session_id")
        except ValueError:
            pass
    return r.get("computer")


def tokens(u):
    cc = u.get("cache_creation") or {}
    w5 = cc.get("ephemeral_5m_input_tokens", 0)
    w1 = cc.get("ephemeral_1h_input_tokens", 0)
    w = u.get("cache_creation_input_tokens", w5 + w1)
    return u.get("input_tokens", 0), w, u.get("cache_read_input_tokens", 0)


PRICE = {"opus": (4.0, 20.0, 0.20, 5.0, 8.0), "sonnet": (2.0, 10.0, 0.10, 2.5, 4.0), "haiku": (0.10, 0.50, 0.01, 0.125, 0.2)}


def priced(u, model):
    """Dollars at API prices (Claude Opus 5.5, Sonnet 5.5, Haiku 5.5 rates by family)."""
    fam = next((f for f in PRICE if f in (model or "")), "opus")
    pi, po, pr, p5, p1 = PRICE[fam]
    cc = u.get("cache_creation") or {}
    return (pi * u.get("input_tokens", 0) + po * u.get("output_tokens", 0) + pr * u.get("cache_read_input_tokens", 0)
            + p5 * cc.get("ephemeral_5m_input_tokens", 0) + p1 * cc.get("ephemeral_1h_input_tokens", 0)) / 1e6


class EnvelopeEstimator:
    """sim.py's E2, online: points of the week per priced dollar, from spans of own calls."""

    def __init__(self, gap=2 * 3600, close=0.03, lookback=14 * DAY, min_spans=2):
        self.gap, self.close, self.lookback, self.min_spans = gap, close, lookback, min_spans
        self.spans, self.k, self.cum, self.week_cum = [], None, 0.0, 0.0
        self.start = None; self.last_t = None; self.last_u = None

    def feed(self, t, u7, cost):
        if self.last_u is not None and u7 + 0.02 < self.last_u:
            self.week_cum = 0.0                            # the week reset
        self.cum += cost; self.week_cum += cost
        if self.start is None or t - self.last_t > self.gap:
            self.start = (t, u7, self.cum)
        elif u7 - self.start[1] >= self.close:
            d_p = self.cum - self.start[2]
            if d_p > 0:
                self.spans.append((t, (u7 - self.start[1]) / d_p, d_p))
            self.start = (t, u7, self.cum)
            recent = sorted((x[1], x[2]) for x in self.spans if x[0] > t - self.lookback)
            if len(recent) >= self.min_spans:
                total, acc = sum(x[1] for x in recent), 0.0
                for ratio, w in recent:                    # 20th percentile weighted by own spend
                    acc += w
                    if acc >= 0.2 * total:
                        self.k = ratio
                        break
        self.last_t, self.last_u = t, u7

    def share(self):
        return self.k * self.week_cum


class Detectors:
    def __init__(self, expected_model=None, uncached_level=0.5, uncached_min_tokens=1_000_000,
                 pace_margin=0.15, pace_hours=6.0, share_pace_per_day=None):
        self.alerts = []
        self.expected = norm(expected_model) if expected_model else None
        self.conv = {}                       # conversation -> state
        self.models_seen = {}                # computer -> {model: last time a conversation asked it}
        self.first_seen = {}
        self.ttls = {}                       # computer -> ttls asked last
        self.pair_seen = set()
        self.hour = defaultdict(deque)       # computer -> (t, uncached, total)
        self.hour_alerted = {}
        self.week = defaultdict(deque)       # computer -> (t, u7)
        self.pace_alerted = {}
        self.uncached_level, self.uncached_min = uncached_level, uncached_min_tokens
        self.pace_margin, self.pace_hours = pace_margin, pace_hours
        self.est = defaultdict(lambda: EnvelopeEstimator())
        self.share_pace = share_pace_per_day
        self.last_headers = {}

    def alert(self, r, kind, text):
        self.alerts.append({"t": r["t"], "computer": r.get("computer"), "kind": kind, "text": text})

    def feed(self, r):
        q, a = r.get("request") or {}, r.get("response") or {}
        comp, ck = r.get("computer"), conv_key(r)
        asked, answered = norm(q.get("model")), norm(a.get("model"))
        u = a.get("usage") or {}
        main = r.get("main", True)

        # 1a: the model that answered is not the model asked (a fact of the request and its reply)
        if answered and asked and answered != asked:
            key = (ck, asked, answered)
            if key not in self.pair_seen:
                self.pair_seen.add(key)
                fb = q.get("fallbacks") or any(it.get("type") == "fallback_message" for it in (u.get("iterations") or []))
                why = "the request allowed a fallback" if fb else "the request named no fallback"
                self.alert(r, "1a model answered", f"asked {asked}, answered by {answered} ({why})")
        st = self.conv.get(ck)
        if main:
            if st is None:
                st = self.conv[ck] = {"model": asked, "prev": None}
                seen = self.models_seen.setdefault(comp, {})
                recent = [m for m, t in seen.items() if t > r["t"] - 7 * DAY]
                first = self.first_seen.setdefault(comp, r["t"])
                if recent and asked not in recent and r["t"] - first > DAY:   # after a first day of learning
                    self.alert(r, "1c new model on computer", f"a conversation starts on {asked}, which this computer has not asked for in 7 days (it asked {', '.join(sorted(recent))})")
                seen[asked] = r["t"]
                if self.expected and asked != self.expected:
                    self.alert(r, "1d not your model", f"asks {asked}; you set {self.expected} for this computer")
            elif asked != st["model"]:
                self.alert(r, "1b program switched", f"the program now asks {asked} in a conversation that asked {st['model']}")
                st["model"] = asked
                self.models_seen.setdefault(comp, {})[asked] = r["t"]

        # 2a: the cache lifetime asked for changed (read from cache_control in the request)
        ttls = tuple(sorted(set(q.get("cache_ttls") or [])))
        if main and ttls:
            before = self.ttls.get(comp)
            if before is not None and ttls != before:
                h = self.last_headers.get(comp) or {}
                hint = ""
                if h.get("anthropic-ratelimit-unified-overage-status") in ("allowed", "allowed_warning") and h.get("anthropic-ratelimit-unified-status") == "rejected":
                    hint = "; the previous reply said calls now draw on extra usage"
                self.alert(r, "2a cache lifetime", f"requests now ask a cache lifetime of {'/'.join(ttls)} instead of {'/'.join(before)}{hint}")
            self.ttls[comp] = ttls

        # 2b: a prefix that was cached and still alive was not read
        inp, w, rd = tokens(u)
        total = inp + w + rd
        if main and st is not None:
            p = st["prev"]
            if p is not None:
                life = 3600 if "1h" in p["ttls"] else 300
                cached = p["rd"] + p["w"]
                rewritten = total < 0.7 * p["total"]           # compaction or edited history
                same_model = p["model"] == answered
                if (r["t"] - p["t"] < life and cached >= 4096 and rd < 0.5 * cached and not rewritten and same_model):
                    self.alert(r, "2b cache not read", f"{cached} tokens cached {r['t'] - p['t']:.0f} s earlier with a {life // 60} min lifetime; {rd} read, {inp + w} paid uncached")
            st["prev"] = {"t": r["t"], "rd": rd, "w": w, "total": total, "ttls": ttls, "model": answered}

        # 2c: uncached share of input over the last hour, per computer
        dq = self.hour[comp]
        dq.append((r["t"], inp + w, total))
        while dq and dq[0][0] < r["t"] - HOUR:
            dq.popleft()
        unc, tot = sum(x[1] for x in dq), sum(x[2] for x in dq)
        over = tot >= self.uncached_min and unc / tot > self.uncached_level
        if over and not self.hour_alerted.get(comp):
            self.alert(r, "2c uncached share", f"{100 * unc / tot:.0f}% of the last hour's {tot / 1e6:.1f}M input tokens were not read from the cache")
        if not over and tot and unc / tot < self.uncached_level * 0.8:
            self.hour_alerted[comp] = False               # re-armed once it falls back clearly
        elif over:
            self.hour_alerted[comp] = True

        # 3a: the week's reported figure is ahead of an even pace through the week by a margin,
        # while this computer is calling the model (the check Claude Code itself makes, at 25% used
        # by 15% of the week, 50% by 35%, 75% by 60%)
        h = r.get("headers") or {}
        if h:
            self.last_headers[comp] = h
        u7 = h.get("anthropic-ratelimit-unified-7d-utilization")
        reset = h.get("anthropic-ratelimit-unified-7d-reset")
        if u7 is not None and reset is not None:
            u7, reset = float(u7), float(reset)
            elapsed = min(1.0, max(0.0, (r["t"] - (reset - 7 * DAY)) / (7 * DAY)))
            ahead = u7 - elapsed
            key = (comp, reset)
            if ahead >= self.pace_margin and not self.pace_alerted.get(key):
                left = (reset - r["t"]) / DAY
                self.alert(r, "3a week ahead of pace", f"the plan's week is {100 * u7:.0f}% used with {100 * elapsed:.0f}% of it gone ({left:.1f} days to reset), while this computer is calling the model")
                self.pace_alerted[key] = True
        # 3b: the computer's own estimated share (an opinion) rising faster than its pace
        if u7 is not None:
            e = self.est[comp]
            e.feed(r["t"], float(u7), priced(u, a.get("model") or q.get("model")))
            if e.k is not None:
                self.feed_share(r["t"], comp, e.share())

    def feed_share(self, t, comp, share):
        """3b: an estimate of the computer's share (an opinion) rising faster than its pace."""
        if self.share_pace is None:
            return
        dq = self.week["share:" + comp]
        dq.append((t, share))
        while dq and dq[0][0] < t - self.pace_hours * HOUR:
            dq.popleft()
        if dq[-1][0] - dq[0][0] >= 0.9 * self.pace_hours * HOUR:
            rate = (share - dq[0][1]) / ((t - dq[0][0]) / DAY)
            k = "share:" + comp
            if rate > 2 * self.share_pace and not self.pace_alerted.get(k):
                self.alerts.append({"t": t, "computer": comp, "kind": "3b share pace (precaution)",
                                    "text": f"this computer's estimated share is rising {100 * rate:.1f} points a day; its pace is {100 * self.share_pace:.1f}"})
                self.pace_alerted[k] = True
            elif rate < self.share_pace:
                self.pace_alerted[k] = False


def main():
    path = sys.argv[1]
    exp = sys.argv[sys.argv.index("--expected-model") + 1] if "--expected-model" in sys.argv else None
    d = Detectors(expected_model=exp, uncached_min_tokens=10_000 if "--small" in sys.argv else 1_000_000)
    for line in open(path):
        d.feed(json.loads(line))
    for al in d.alerts:
        print(f"{al['t']:.0f}  {al['kind']:<28} {al['text']}")
    if not d.alerts:
        print("no alert")


if __name__ == "__main__":
    main()
