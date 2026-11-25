"""The fix: the helper keeps main's clock on the Mac's.

Two triggers, as the tool in #378 does it:
- on wake: the Mac tells the helper it woke (NSWorkspace.didWakeNotification,
  or IOKit's kIOMessageSystemHasPoweredOn; or the helper sees
  mach_continuous_time - mach_absolute_time grow, the pair #370 already uses
  for the record). The helper steps main's clock at once;
- every `period` seconds: it reads main's offset and steps when it exceeds
  `limit`, which catches what the wake signal misses (a wake the helper slept
  through, a guest that drifts while running).
The step goes over the helper's own way into main: on the Mac, the SSH
connection with Silo's key and sudo that #370's step 2 already opens
(`sudo date -u <MMDDhhmmCCYY.ss>`, whole seconds); here, a serial line to a
small agent in the guest, which sets nanoseconds.
"""
import threading


class ClockKeeper:
    def __init__(self, guest, period=60.0, limit=0.5, log=None):
        self.guest, self.period, self.limit = guest, period, limit
        self.log = log if log is not None else []
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._loop, daemon=True)
        self.thread.start()

    def _check(self, why):
        with self.lock:
            off, _ = self.guest.offset()
            if abs(off) > self.limit:
                after, _ = self.guest.set_clock()
                self.log.append((why, off, after))

    def woke(self):
        """Called by the wake signal."""
        self._check("wake")

    def _loop(self):
        while not self.stop.wait(self.period):
            self._check("period")

    def close(self):
        self.stop.set(); self.thread.join()
