"""N, median, min and max of the last number on each line of a file.

    python3 measure_stats.py LABEL FILE [UNIT]
"""
import statistics
import sys

label, path = sys.argv[1], sys.argv[2]
unit = sys.argv[3] if len(sys.argv) > 3 else 's'
values = [float(p[-1]) for p in (line.split() for line in open(path)) if p]
if not values:
    print(f'{label}: no data')
    sys.exit(1)
print(f'{label}: N={len(values)} median={statistics.median(values):.2f}{unit} '
      f'min={min(values):.2f}{unit} max={max(values):.2f}{unit} runs={[round(v, 2) for v in values]}')
