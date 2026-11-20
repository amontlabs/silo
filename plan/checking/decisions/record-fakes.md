# Which fakes the simulator adds for the record

What each fake tries to break:

- The source that drops, repeats and delays: `every-loss-is-a-gap`, in the part `record`.
- The inside that lies about which process did something: no rule reads a report, so no rule
  changes its decision (`rule-reads-only-facts`, in the part `rules`).
- The computer that floods: `flood-never-removes-older-lines`, in the part `record`.
