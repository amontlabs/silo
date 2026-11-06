"""N, median, min and max of the numbers in a file (one per line, optional label first).

    python3 final_stats.py LABEL FILE [UNIT]
"""
import statistics
import sys

label, path = sys.argv[1], sys.argv[2]
unit = sys.argv[3] if len(sys.argv) > 3 else 's'
values = []
for line in open(path):
    parts = line.split()
    if parts:
        values.append(float(parts[-1]))
if not values:
    print(f'{label}: no data')
    sys.exit(1)
print(f'{label}: N={len(values)} median={statistics.median(values):.2f}{unit} '
      f'min={min(values):.2f}{unit} max={max(values):.2f}{unit} runs={[round(v, 2) for v in values]}')
