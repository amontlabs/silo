"""Which prices a new release of the table lowers, against the table the app holds.

A price a release lowers lets a budget let more through, so the app takes a cut beyond `floor`
(half, by default) only from a release the root key signed too. A raise, a new model and a cut
within the floor need only the online key. A removed row falls back to the bound (#396: twice the
dearest price of its provider), which is compared like any other price.

Prices are compared per provider, model and tier, on today and on every later date of effect in
either table, with the multipliers that apply to every request (`when` empty) folded in. A
multiplier that applies only when the request asks for it (US-only inference) is compared as a
factor on its own."""
FIELDS = ("input", "output", "cache_read", "cache_write_short", "cache_write_long",
          "web_search_per_1k", "cache_storage_per_hour")
MARGIN = 2.0


def _match(pattern, model):
    return model.startswith(pattern[:-1]) if pattern.endswith("*") else model == pattern


def pick(rows, provider, model, tier, day):
    """#396's `pick` in prices.zig: an exact id beats a prefix, then the latest date of effect."""
    best = None
    for r in rows:
        if r["provider"] != provider or not _match(r["model"], model) or r.get("tier", "standard") != tier: continue
        if r.get("from", "2000-01-01") > day: continue
        if best is not None:
            b_exact, r_exact = not best["model"].endswith("*"), not r["model"].endswith("*")
            if b_exact and not r_exact: continue
            if r_exact == b_exact and r.get("from", "2000-01-01") <= best.get("from", "2000-01-01"): continue
        best = r
    return best


def _mult(mults, provider, model, when, day):
    f = 1.0
    for m in mults:
        if m["provider"] != provider or m.get("when", "") != when: continue
        if m.get("from", "2000-01-01") > day: continue
        if m.get("models") and not any(_match(p, model) for p in m["models"]): continue
        f *= m["factor"]
    return f


def bound(rows, provider, tier, day):
    """#396's bound: twice the dearest price of the provider and tier, field by field."""
    out = {}
    for f in FIELDS:
        vals = [r.get(f, 0) for r in rows if r["provider"] == provider and r.get("tier", "standard") == tier and r.get("from", "2000-01-01") <= day]
        out[f] = MARGIN * max(vals, default=0)
    return out


def effective(table, key, day):
    """The prices of one call to model key[1] on `day`, and whether a row (not the bound) gave them."""
    r = pick(table.get("rows", []), key[0], key[1], key[2], day)
    base = {f: r.get(f, 0) for f in FIELDS} if r else bound(table.get("rows", []), key[0], key[2], day)
    m = _mult(table.get("multipliers", []), key[0], key[1], "", day)
    return {f: v * m for f, v in base.items()}, r is not None


def cuts(old, new, today, floor=0.5):
    """Every price `new` sets below `floor` times what `old` sets, from today on."""
    # Probe every model id either table names; a prefix row `x*` is probed with an id it alone matches.
    keys = {(r["provider"], r["model"][:-1] + "~probe" if r["model"].endswith("*") else r["model"], r.get("tier", "standard"))
            for t in (old, new) for r in t.get("rows", [])}
    days = sorted({today} | {r.get("from", "2000-01-01") for t in (old, new) for r in t.get("rows", []) + t.get("multipliers", []) if r.get("from", "2000-01-01") > today})
    found = []
    for key in sorted(keys):
        for d in days:
            o, o_known = effective(old, key, d)
            n, n_known = effective(new, key, d)
            if not o_known and n_known:
                continue  # a model the old table did not price: see README, "What the app cannot police"
            for f in FIELDS:
                if o[f] > 0 and n[f] < floor * o[f]:
                    found.append({"key": "/".join(key), "day": d, "field": f, "old": o[f], "new": n[f]})
    # conditional multipliers (US-only inference): compared as factors
    conds = {(m["provider"], m.get("when", "")) for t in (old, new) for m in t.get("multipliers", []) if m.get("when")}
    for key in sorted(keys):
        for prov, when in conds:
            if prov != key[0]: continue
            for d in days:
                of, nf = _mult(old.get("multipliers", []), prov, key[1], when, d), _mult(new.get("multipliers", []), prov, key[1], when, d)
                if nf < floor * of:
                    found.append({"key": "/".join(key), "day": d, "field": f"multiplier {when}", "old": of, "new": nf})
    return found
