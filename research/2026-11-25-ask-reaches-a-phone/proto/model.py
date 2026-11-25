"""How long until a person answers an ask on the phone, and which holds that time fits.

A model, not a measurement of people: each context's distribution is set from one published
figure (cited in the README) and the shape around it is a guess, so the output is given for a
narrow and a wide spread. What it answers: for each kind of hold, the share of asks answered
while the request still waits, the share answered later but within the ask's own life (a late
yes the agent's retry can use), and the share left for the morning.

    python3 model.py > ../results/model.json
"""
import json, math, random

random.seed(7)
N = 200_000

# Each context: how the ask reaches the person, and the median wait to an answer, in seconds.
CONTEXTS = {
    # Duo push in a university: median 6.67 s from push to approval (DuoLungo, 2026).
    "at the Mac, or phone in hand": {"median": 7, "spread": (0.4, 0.8)},
    # Messages attended within a median of 6.15 minutes (Pielot et al., CHI 2014, as cited by
    # Katevas et al. 2017).
    "away, awake, phone nearby": {"median": 369, "spread": (1.0, 1.6)},
    # A meeting, a film, Do Not Disturb: notifications batched. Okoshi et al. report a mean
    # response of 54 min, 27 with their scheduling; we take a median of 30 min.
    "busy or Do Not Disturb": {"median": 1800, "spread": (0.8, 1.2)},
}
# Delivery of the push itself: about 0.8 s mean and 2 s at the 95th percentile for APNs VoIP
# (Apple developer forums); a tail of minutes happens on a bad connection.
DELIVERY = {"median": 0.8, "spread": 0.55, "tail_share": 0.01, "tail": (60, 300)}
GATHER = 2.0          # `asks-for-a-burst`: the ask waits 2 s to gather its burst

HOLDS = {             # the proposed `hold-limit-length`, per client
    "pip (gives up at 15 s)": 12,
    "aws, boto3, Railway CLI (60 s)": 50,
    "unknown client (90 s)": 90,
    "git push, terraform, model reply kept alive": 600,
}
ASK_LIFE = {"30 min (#355)": 1800, "8 h (a night)": 8 * 3600}


def lognorm(median, sigma):
    return median * math.exp(random.gauss(0, sigma))


def delivery():
    if random.random() < DELIVERY["tail_share"]:
        return random.uniform(*DELIVERY["tail"])
    return lognorm(DELIVERY["median"], DELIVERY["spread"])


def sample(ctx, wide):
    c = CONTEXTS[ctx]
    return GATHER + delivery() + lognorm(c["median"], c["spread"][1 if wide else 0])


def asleep():
    # An ask at a uniform time of an 8-hour night is answered at wake-up, plus a few minutes.
    return random.uniform(0, 8 * 3600) + lognorm(300, 0.8)


def table(draw):
    xs = sorted(draw() for _ in range(N))
    q = lambda p: xs[int(p * (N - 1))]
    row = {"median_s": round(q(0.5), 1), "p90_s": round(q(0.9), 1)}
    for name, h in HOLDS.items():
        row[f"answered within hold: {name}"] = round(sum(x <= h for x in xs) / N, 3)
    for name, life in ASK_LIFE.items():
        row[f"answered within ask life {name}"] = round(sum(x <= life for x in xs) / N, 3)
    return row


out = {"model": {"contexts": CONTEXTS, "delivery": DELIVERY, "gather_s": GATHER, "holds_s": HOLDS},
       "results": {}}
for ctx in CONTEXTS:
    for wide in (False, True):
        out["results"][f"{ctx} ({'wide' if wide else 'narrow'} spread)"] = table(lambda: sample(ctx, wide))
out["results"]["asleep"] = table(asleep)

# A night of agent work seen from the phone: the person is awake and away for the first 3 hours,
# then asleep for 8. An ask comes at a uniform time over those 11 hours.
def night():
    t = random.uniform(0, 11 * 3600)
    if t < 3 * 3600:
        x = sample("away, awake, phone nearby", True)
        return x if t + x < 3 * 3600 else (3 * 3600 - t) + asleep_from(0)
    return asleep_from(t - 3 * 3600)


def asleep_from(t_into_sleep):
    return (8 * 3600 - t_into_sleep) + lognorm(300, 0.8)


out["results"]["an evening then a night (3 h away, 8 h asleep)"] = table(night)

# How many asks reach the phone: #405's settled labels of #383's 1,085 documented writes, with the
# wording of `outcome-of-a-call` in force. These are documented operations, not traffic: each
# operation counts once, however often agents call it.
should, held_right, should_pass, held_wrong, revocations = 267, 240, 575, 125, 66
asks = held_right + held_wrong
out["asks_from_405"] = {
    "held per 100 documented writes, rules as merged": round(100 * asks / (should + should_pass), 1),
    "share of asks that are wrong holds, as merged": round(held_wrong / asks, 3),
    "held per 100, revocations passing": round(100 * (asks - revocations) / (should + should_pass), 1),
    "share of asks that are wrong holds, revocations passing": round((held_wrong - revocations) / (asks - revocations), 3),
    "missed (should be held, pass)": should - held_right,
}
print(json.dumps(out, indent=1))
