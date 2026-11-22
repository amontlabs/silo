# When an extension's computer runs

## For the owner

An agent's part, 22 November 2026. You install two extensions. Disk watch wakes every minute to read
/work on main. Request check is asked about each request of main that waits at the boundary.

The options:

- Running from install until you remove it (recommended). Between wakes each extension's computer is
  idle and gives memory back, so a request waits only for the answer, not for a boot. It costs room
  on your Mac: a Mac holds a few running computers, not fifty, and each extension's computer is one
  of them. If one crashes, Silo starts it once more and tells you; if it crashes again, it stays
  stopped, and its wakes are skipped and recorded.
- Started at each wake, stopped after. It costs a boot at each wake: every request Request check is
  asked about waits for one, and Disk watch boots sixty times an hour.
- Started at a wake, stopped after some idle minutes. It costs a boot at the first wake after a
  quiet spell, and the request that comes first waits for it.

Recommended: running from install. An extension that answers a waiting request has to be there when
the request comes, and an idle computer gives its memory back.

## Notes

Split on 22 November 2026 from `extension-registration`, which keeps how Silo keeps an extension
and connects its program to its tools.
