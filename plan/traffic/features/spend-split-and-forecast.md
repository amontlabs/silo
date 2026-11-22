# Spend split, and a forecast of a plan's limits

## The choice

An agent's part, 22 November 2026. This week main spent 140 dollars. Silo shows that 60 of them went
to reading cached context, and 90 to one model at its highest effort. At this pace, it says, your
plan's weekly limit runs out on Thursday.

The options:

- Build it (recommended), as an extension. You see what drives your spend and when your plan will
  run out. It costs a forecast that is a guess, shown as one, and wrong after any change of pace.
  Where Silo learns all of a plan's limits is still to find out.
- Leave it out. You see totals only. It costs working out yourself what drives them, as people do
  today with separate tools.

Recommended: build it. Running out of a plan in the middle of the week is a common surprise, and the
numbers behind it are already in the traffic Silo reads.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- The split comes from the model calls read at the boundary. `how-spend-is-shown` proposes that
  spend is never shown as raw tokens, so each share is in dollars or as a share of a plan.
- The forecast is an opinion and is shown as one (`kinds-look-different`).
- A plan's limits, and how much of them is used, are not all in the traffic. Where Silo learns them
  is to find out.
- An extension that reads the record and shows a panel could do this.
