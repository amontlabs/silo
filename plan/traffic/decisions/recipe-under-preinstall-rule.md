# Does a recipe fall under Anthropic's rule for a product that preinstalls or runs Claude Code

## For the owner

An agent's part, 22 November 2026. Silo's recipe for main fetches Claude Code from Anthropic when
main is made, and you sign in with your Max plan. Anthropic's rule says that preinstalling or
running Claude Code in a product requires agreeing to its Commercial Terms. Whether a recipe that
runs on your own Mac counts is not clear.

The options:

- Act as if it does (recommended). Silo agrees to Anthropic's Commercial Terms, the ones businesses
  accept to use its API, and keeps the rule's two conditions. Silo still asks Anthropic. It costs
  being bound by those terms, such as not reverse engineering Anthropic's services, even if the rule
  never applied.
- Ask first. Silo ships no recipe that fetches Claude Code until Anthropic answers. It costs a main
  without Claude Code ready for as long as the answer takes, and it may never come.
- Ship no such recipe. You add Claude Code to your recipes yourself. It costs every user that step.

Recommended: act as if it does. It costs little, and Silo is then in order under either reading.

## Notes

To find out, by asking. The rule is in the notes of `anthropic-conditions-kept`.
