# When a check counts

A check that has never failed shows nothing: it may be looking at the wrong thing, or at nothing.

One example, from the trial described in the notes of `core-simulator`, 21 November 2026. The
code that gives a secret was changed on purpose so that a precaution could give one. The simulator
found it within 3 histories on average and printed the one call that shows it. Had it found
nothing, the check would have been the thing to fix.

The standard name is mutation testing. What differs here: one fault is put in by hand for each
check, where mutation testing changes the code automatically in many places.

The owner said yes on 21 November 2026.
