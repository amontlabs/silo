# Phone app

## The choice

An agent's part, 22 November 2026. You are away from your Mac at 18:40. An agent in main asks to
push to the default branch of your repository, and the ask ends in 90 seconds.

The options:

- Build it (recommended). Your phone shows the ask and your alerts, and you answer the ask there. It
  costs a second app to keep in step, and a way to reach Silo on your Mac from outside your home
  network, which has to be built with care since a yes from the phone grants.
- Leave it out. Asks wait in the app on your Mac. It costs every ask that comes while you are away:
  it is refused when its time runs out, and the agent stops or works around it.

Recommended: build it, when its stage comes. An ask has to reach you where you are, and holding
merges and deploys for your yes already counts on it.

## Reaching the Mac from outside its network (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-ask-reaches-a-phone/`. The "way to reach Silo on your Mac from outside
your home network" needs no port opened on the Mac: the Mac and the phone each make only outbound
requests to a relay, and everything they send is sealed and signed, so the relay cannot answer
for you. On Linux the ask reached the stand-in phone 2 seconds after the hold, 2 of which are the
wait for a burst, and the yes released the request 20 ms after the phone sent it.

To rule: an iPhone app gets pushes only from a server holding its publisher's push key, so the
phone app needs a relay Amont Labs runs. It would see a sealed message's size and time, not what
the ask says. See the notes of `what-silo-sends-home`.

A Mac that sleeps reads no answer. The relay keeps the answer until the Mac wakes, so an answer
should stay valid for as long as its ask is open.
