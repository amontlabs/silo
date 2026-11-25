import json, os, re, sys
progs = ["cc", "cx", "gem", "oc", "cl", "ai", "go", "cob"]
vars_ = ["400", "401", "402", "403", "403noretry", "404", "413", "429ra", "451", "500", "200"]
print("| program | " + " | ".join(vars_) + " |"); print("|---" * (len(vars_) + 1) + "|")
for p in progs:
    row = []
    for v in vars_:
        d = f"out/{p}-refuse{v.replace('noretry', ':noretry').replace('ra', ':ra') if v[-1] in 'ay' else v}-store"
        if not os.path.exists(d + "/result.txt"): row.append("…"); continue
        reqs = [json.loads(l) for l in open(d + "/requests.jsonl")] if os.path.exists(d + "/requests.jsonl") else []
        model = [r for r in reqs if r.get("kind")]
        res = open(d + "/result.txt").read(); ex = re.search(r"exit=(\d+) secs=([\d.]+)", res)
        txt = open(d + "/out.txt", errors="replace").read() + open(d + "/err.txt", errors="replace").read()
        shown = "budget is reached" in re.sub(r"\s+", " ", txt)
        last = max([r["t"] for r in model], default=0)
        n = len(model); stopped = ex.group(1) != "124"
        row.append(f"{n}{'' if stopped else '+'} {'✓' if shown else '✗'} {float(ex.group(2)):.0f}s")
    print(f"| {p} | " + " | ".join(row) + " |")
