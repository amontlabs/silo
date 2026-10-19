---
"silo-ui": patch
---

- Silo opens faster: live updates, saved settings and the application list now load in parallel, and the window code loads on demand.
- Fewer background reads: returning to the window, saving a secret and network or SSH polling no longer repeat state reads that were just done, and network services are read only where they are shown.
- The operation list recovers on its own if a read fails, and one stalled state read no longer stops Silo from refreshing.
