import { readFileSync } from "node:fs"
import { expect, it } from "vitest"

it("directs connected-computer checkpoint deletion to the owning device", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Delete checkpoint data"))
  expect(instructions?.textContent).toMatch(/on the (?:computer's )?owning device/i)
})

it("qualifies Safari computer hostname support for older supported macOS versions", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.querySelector("strong")?.textContent === "Network")
  expect(instructions?.textContent).toMatch(/Safari.*macOS 26/)
  expect(instructions?.textContent).toMatch(/Chrome or Firefox/)
  expect(instructions?.textContent).not.toMatch(/every browser/i)
  expect(instructions?.textContent).toMatch(/127\.0\.0\.1.*shares.*cookie/i)
})

it("qualifies copying diagnostics because some Details sections have no copy control", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Read the operation's error"))
  expect(instructions?.textContent).toMatch(/copy control when.*offered/i)
})

it("distinguishes reclaimed host allocation from unchanged workspace capacity", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Free up space"))
  expect(instructions?.textContent).toMatch(/reclamation.*allocated space.*on this device/i)
  expect(instructions?.textContent).toMatch(/workspace capacity.*stay.*same/i)
})

it("qualifies duplicated desktop settings for new computers with built-in computer use", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Duplicate settings"))
  expect(instructions?.textContent).toMatch(/built-in computer use.*desktop.*starts automatically/i)
  expect(instructions?.textContent).toMatch(/even if.*original.*manual/i)
})

it("directs system-issue recovery through the displayed instructions and Retry checks", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Read the operation's error"))
  expect(instructions?.textContent).toMatch(/System issue.*recovery instructions.*Retry checks/i)
  expect(instructions?.textContent).not.toMatch(/relaunch Silo to rerun startup checks/i)
})

it("explains checkpoint deletion blockers and cleanup when dependencies are removed", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Delete checkpoint data"))
  expect(instructions?.textContent).toMatch(/latest state.*builds on/i)
  expect(instructions?.textContent).toMatch(/later checkpoints/i)
  expect(instructions?.textContent).toMatch(/dependencies.*removed/i)
  expect(instructions?.textContent).not.toMatch(/until its last dependent computer is deleted/i)
})

it("qualifies update instructions because Silo Dev has no release feed", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("With automatic checks enabled"))
  expect(instructions?.textContent).toMatch(/production Silo app/i)
  expect(instructions?.textContent).toMatch(/Silo Dev has no update feed/i)
})

it("explains the update prerequisite before starting an outdated legacy desktop", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.startsWith("Start desktop with computer"))
  expect(instructions?.textContent).toMatch(/Update desktop.*then.*Start desktop/i)
  expect(instructions?.textContent).toMatch(/computer running.*desktop stopped/i)
  expect(instructions?.textContent).toMatch(/updating.*does not start.*automatically/i)
})

it("names the macOS Settings menu for both Silo build channels", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.includes("Search or Jump To"))
  expect(instructions?.textContent).toMatch(/Silo → Settings/)
  expect(instructions?.textContent).toMatch(/Silo Dev → Settings/)
})

it("says agents, including ones installed later, are set up for computer use automatically", () => {
  const help = new DOMParser().parseFromString(readFileSync("docs/silo-help.html", "utf8"), "text/html")
  const instructions = [...help.querySelectorAll("p")].find(paragraph => paragraph.textContent?.startsWith("Agents are set up automatically"))
  expect(instructions?.textContent).toMatch(/Claude Code and Codex, including before they are installed/i)
  expect(instructions?.textContent).toMatch(/registers a newly installed Claude Code or Codex/i)
  expect(help.body.textContent).not.toMatch(/Set up computer use for new agents/)
  expect(help.body.textContent).not.toMatch(/Pi, OMP and Hermes|Set up LCU/)
})
