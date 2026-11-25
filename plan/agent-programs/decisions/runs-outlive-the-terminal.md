# What keeps an agent running after the terminal closes

## The choice

An agent's part, 24 November 2026. At 23:00 you run `ssh main`, start Claude Code, and close the
laptop's terminal. The SSH session ends, and with it the shell and Claude Code. In the morning
nothing ran.

The options:

- A second SSH entry that opens tmux (recommended). `ssh main.keep` attaches to a tmux session in
  main, or makes one. Each agent gets its own window. It costs a second entry per computer, which
  changes "one named entry per computer" in `reaching-a-computer`.
- Say nothing, and let people use tmux or `nohup` themselves. It costs the first night of everyone
  who does not.
- Silo starts agents itself, as tasks. It costs the first build: tasks need recipes and receipts.

Recommended: a second SSH entry that opens tmux. It is what people who run agents overnight already
do by hand.

## Notes

Agent, 24 November 2026. From draft #354, gap 1 of `details/daily-loop.md`. tmux comes from
`main-set-up-for-agents`.
