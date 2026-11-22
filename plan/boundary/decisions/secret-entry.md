# Where does the user enter a secret

## For the owner

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
