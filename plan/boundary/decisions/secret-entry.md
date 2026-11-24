# Where does the user enter a secret

## The choice

An agent's part, 22 November 2026. An agent in main needs your Stripe key. Today you would paste it
into the chat, where it goes to the model provider and stays in the conversation.

The options:

- Silo's own prompt (recommended). The agent asks for the key by name, Silo shows you a prompt that
  says which computer asked, and you type the value and choose where it may be sent. It costs one
  step outside the chat. Silo also cannot stop you from pasting a key into a chat anyway.
- Silo's prompt, and reading from where you keep them. Silo can also take a value from your Mac's
  keychain or a password manager. It costs more to build, and Silo then touches the place where all
  your passwords are.

Recommended: Silo's own prompt. It is enough to keep the value out of every chat, and reading from a
password manager can come later as a feature of its own.

## A key pasted into a chat can be recognised before it leaves (24 November 2026)

An agent's part, 24 November 2026. The answer's cost is that "Silo also cannot stop you from
pasting a key into a chat anyway". Silo reads model requests, so it sees a pasted key on its way to
the provider, and many keys say what they are by their form: `sk_live_` is Stripe, `ghp_` is
GitHub, `xoxb-` is Slack. Tried with the rules of gitleaks, a common open-source secret scanner:
222 rules, of which 120 recognise a key by its form alone.

- **On ordinary agent traffic they stay quiet.** Over 182 model requests that Claude Code and
  Codex sent in this project's trials (9 MB), the 120 rules found nothing but one test value
  planted earlier. The other 101 rules, which look for a name such as `api_key` near a random
  string, fired 112 times, all on an id Codex sends with each request. So only rules that know a
  key's form are fit for this.
- **On a pasted key they find it.** A prompt to Claude Code held 16 made-up keys in the forms of
  common services. Each went to the provider as typed, inside the request's JSON, and the 14 with a
  known form were all found: Anthropic, OpenAI, GitHub (both kinds), Slack, Stripe, AWS, Hugging
  Face, npm, SendGrid, GitLab, Linear, Notion, PyPI. A Supabase key of its new form and a token with
  no prefix were not.

What I would add to the answer: a model request that holds a key of a known form is held, with an
ask that names the service ("This message to Anthropic holds what looks like a Stripe live key").
You can send it as it is, or keep the key in Silo, which then knows the host the key belongs to
from its form. Keeping it means sending a placeholder where you typed the key, in this request and
every later one of the conversation, which is a second change to model traffic beside the swap.
`what-silo-changes-in-traffic` allows only the swap, so that part is for the owner. The scripts
are in the project files, `research/2026-11-24-pasted-keys`.
