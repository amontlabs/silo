"""Does a computer's process hold the Mac awake? Reads `pmset -g assertions` on stdin.

    pmset -g assertions | assertions.py PID...      prints yes or no, then the matching lines
    assertions.py --selftest

A process holds the Mac awake when it owns a PreventUserIdleSystemSleep or PreventSystemSleep
assertion (the kinds that stop the system sleeping, not only the display). The computer's processes
are the PIDs given (silovm run) and every com.apple.Virtualization process, which is where Apple's
framework would take one.
"""
import re
import sys

LINE = re.compile(r"pid (\d+)\(([^)]*)\):.*?\b(PreventUserIdleSystemSleep|PreventSystemSleep|NoIdleSleepAssertion)\b")


def holds(text, pids):
    hits = []
    for line in text.splitlines():
        m = LINE.search(line)
        if m and (m.group(1) in pids or "Virtualization" in m.group(2)):
            hits.append(line.strip())
    return hits


SAMPLE = """Assertion status system-wide:
   PreventUserIdleSystemSleep     1
Listed by owning process:
   pid 512(caffeinate): [0x0000a1b2] 00:10:00 PreventUserIdleSystemSleep named: "caffeinate command-line tool"
   pid 700(com.apple.Virtualization.VirtualMachine): [0x0000a1b3] 00:09:58 PreventUserIdleSystemSleep named: "Virtual machine running"
   pid 701(silovm): [0x0000a1b4] 00:09:58 PreventUserIdleDisplaySleep named: "display"
"""


def main():
    if sys.argv[1:] == ["--selftest"]:
        ok = (len(holds(SAMPLE, {"701"})) == 1 and holds(SAMPLE, set())[0].startswith("pid 700")
              and not holds(SAMPLE.replace("Virtualization", "Other"), {"701"}))
        print("assertions: %s" % ("ok" if ok else "FAIL"))
        sys.exit(0 if ok else 1)
    hits = holds(sys.stdin.read(), set(sys.argv[1:]))
    print("yes" if hits else "no")
    for h in hits:
        print(h)


if __name__ == "__main__":
    main()
