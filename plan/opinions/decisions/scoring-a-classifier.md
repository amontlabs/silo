# Scoring a classifier

## For the owner

An agent's part, 22 November 2026. Silo's classifier that says how a path would come back gets
version 1.3. Over two months you answered 40 of its questions. Version 1.2 agrees with 39 of your
answers, and 1.3 with 37.

The options:

- Kept back when it does worse (recommended). 1.2 stays, and the app tells you what 1.3 would
  change. You can switch anyway. It costs waiting: whatever 1.3 does better elsewhere waits for you.
  Trying it on your record also takes time, and tokens for a classifier that asks a model.
- Always replaced, with the score shown. 1.3 takes over and the app shows 37 of 40. It costs a
  version that does worse on your own work going live before you notice.
- Not tried on your answers. Silo tests its classifiers before each release, on other people's
  examples. It costs classifiers tuned to work that is not yours.

Recommended: kept back when it does worse. Your answers are the only examples of what right means
for your work, so a version that gets more of them wrong should not take over on its own.

## Notes

The design record marks this both as the owner's and as a proposal, and does not say which
sentence is whose.

The row "A classifier's opinion" of the table "Where each kind of checking goes", whole:

| What | Is there an exact statement of "right"? | How it is checked |
|---|---|---|
| A classifier's opinion | No: "is this folder a build?" has no exact answer | Scored against the user's answers, and compared with the previous version on recorded history |
