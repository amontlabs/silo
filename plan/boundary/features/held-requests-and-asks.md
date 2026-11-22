# Held requests and asks

## For the owner

An agent's part, 22 November 2026. A rule on main holds every push to github.com. An agent pushes
three commits to the main branch of your site. Only the push waits, the agent's other work goes on,
and you get an ask in Silo that shows the repository, the branch and the three commits.

The options:

- Build it (recommended). Your yes lets that one push pass. With no answer before the hold's limit,
  the push is refused as held and not answered. It costs your attention: each rule that holds is a
  source of interruptions, and people who are asked often click yes. It is also the largest feature
  of this part, with two questions still open: how long a hold lasts, and how a held request is
  refused on each kind of connection.
- Leave it out. A rule allows or denies and nothing in between. It costs you the middle case: what
  you want to see first is denied, you change the rule by hand, and the agent tries again.

Recommended: build it. A push, a publish or a payment is where a person wants one look, and a hold
that always ends is the way to give it without stopping the computer.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It is large: it needs 15 decisions. Cutting it would leave a hold with no limit, which `hold-limit` rules out.
