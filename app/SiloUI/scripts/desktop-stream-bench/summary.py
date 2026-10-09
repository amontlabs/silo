#!/usr/bin/env python3
"""Print a one-screen summary of one or more `run.py` reports."""
import json
import sys

KEYS = ('fps', 'encoded_fps', 'mbps', 'rtt_ms', 'encode_ms', 'decode_ms', 'pipeline_ms', 'cpu_percent')


def mean(samples, key):
    values = [sample[key] for sample in samples if isinstance(sample.get(key), (int, float))]
    return round(sum(values) / len(values), 2) if values else None


def main():
    for path in sys.argv[1:]:
        report = json.load(open(path))
        page = report.get('page', {})
        print(f"== {report.get('label', path)}")
        if 'error' in report:
            print('error:', report['error'])
            continue
        info = page.get('page', {})
        sink = page.get('sink') or {}
        client = page.get('streamClient') or {}
        print(f"page {info.get('innerWidth')}x{info.get('innerHeight')} dpr {info.get('devicePixelRatio')} "
              f"hidden {info.get('hidden')} | stream {client.get('resolution')} {client.get('decoder')} "
              f"({client.get('decoder_evidence')}) via {client.get('sink')} | sink css "
              f"{sink.get('cssWidth')}x{sink.get('cssHeight')} backing {sink.get('backingWidth')}x{sink.get('backingHeight')}")
        print('input to picture ms:', page.get('inputToPicture'))
        for phase in page.get('phases', []):
            print(f"  {phase['name']:6} raf {round(phase['rafPerSecond'])}",
                  {key: mean(phase['samples'], key) for key in KEYS})
        guest = {k: v for k, v in (report.get('guest') or {}).items() if k != 'keyReceivedAt'}
        print('guest (fixture probes + motion):', guest)
        if report.get('idleGuest'):
            print('guest (desktop idle):', report['idleGuest'])
        host = sorted(report.get('hostProcesses', []), key=lambda p: -p['cpuPercent'])[:5]
        print('host (fixture):', [(round(p['cpuPercent'], 1), p['name'].rsplit('/', 1)[-1]) for p in host])
        idle_host = sorted(report.get('idleHostProcesses') or [], key=lambda p: -p['cpuPercent'])[:5]
        if idle_host:
            print('host (desktop idle):', [(round(p['cpuPercent'], 1), p['name'].rsplit('/', 1)[-1]) for p in idle_host])
        starts = page.get('probeStarts') or []
        keys = (report.get('guest') or {}).get('keyReceivedAt') or []
        totals = page.get('inputToPictureSamples') or []
        if starts and len(starts) == len(keys) == len(totals):
            # The guest clock's offset is unknown, so the input share is shown
            # relative to the fastest probe.
            raw = [key - start for key, start in zip(keys, starts)]
            floor = min(raw)
            print('per probe (total, input above fastest):',
                  [(round(total), round(value - floor)) for total, value in zip(totals, raw)])
        if page.get('errors'):
            print('errors:', page['errors'][:5])


if __name__ == '__main__':
    main()
