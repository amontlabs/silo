"""One line per run: what Silo would show for it after reconcile.py. usage: table.py <dir-of-runs>"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import reconcile as R

d = sys.argv[1]
print("| Run | Calls billed | Exchanges seen | Events | Matched | Exchanges with no event | Events with no exchange | Flags | Report's state | Billed | Reported | Floor for the rest | Estimate by bytes | Estimate by the conversation | Billed for the rest |")
print("|---|---:|---:|---:|---:|---:|---:|---|---|---:|---:|---:|---:|---:|---:|")
for name in sorted(os.listdir(d)):
    if not os.path.isdir(f"{d}/{name}"):
        continue
    r = R.reconcile(f"{d}/{name}")
    est = r["usd_estimate_unreported"]
    print(f"| {name} | {r['calls_served']} | {r['model_exchanges']} | {r['events']} | {r['matched']} | {r['exchanges_without_event']} | "
          f"{r['events_without_exchange']} | {', '.join(r['flags']) or '-'} | {r['state']} | {r['usd_billed']:.4f} | {r['usd_reported']:.4f} | "
          f"{r['usd_floor_unreported']:.4f} | {'-' if est is None else f'{est:.4f}'} | {r['usd_estimate2_unreported']:.4f} | {r['usd_billed_unreported']:.4f} |")
