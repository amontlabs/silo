## Proposed, not confirmed by the owner

Why: Memory decides how many computers fit on a Mac, and an agent spends most of its time waiting on a model.

### For the owner to settle

- Is an idle computer paused whole, or kept running with its unused memory handed back?
  - Recommendation: Hand back unused memory first, since pausing whole can break an agent's open connection to its model.

### For whoever builds it

- Detail: Idle is read from the agent's terminal signal (OSC 7501), or from no processor use and no traffic for a set time.
- To find out: How much memory an idle Linux computer holds after a build, and how much the VM runtime can give back without pausing it.
- Test: Make forks until resource-floors refuses one, let them go idle, and the refused fork then starts.
