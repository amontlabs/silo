# Storing what is sent to a model provider

Agreed as a direction. Every number around it is a rough estimate, to find out
(`record-disk-size`).

The trap it avoids: when a provider format resends the conversation history, each request
repeats the whole conversation so far, and raw traffic grows with the square of a conversation's
length.
