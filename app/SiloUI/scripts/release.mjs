import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { execFileSync } from "node:child_process"

import { preflight } from "./preflight.mjs"
import { assertPreStable } from "./sync-release.mjs"

const app = resolve(dirname(fileURLToPath(import.meta.url)), "..")

export function release(action, root = app, run = (command, args) => execFileSync(command, args, { cwd: root, encoding: "utf8", stdio: ["inherit", "pipe", "inherit"] }).trim(), { allowStable = false } = {}) {
  if (!["draft", "publish"].includes(action)) throw new Error("Use npm run release:draft or npm run release:publish.")
  if (run("git", ["status", "--porcelain"])) throw new Error("Commit or stash your changes before releasing. No tag was pushed.")
  const version = run("python3", ["scripts/validate-release-version.py"])
  assertPreStable(version, allowStable)
  const tag = `v${version}`
  const pending = readdirSync(resolve(root, ".changeset")).filter(name => name.endsWith(".md") && name !== "README.md")
  if (pending.length) throw new Error("Unreleased changesets remain. Run npm run release:version, review, and commit first.")
  const notes = resolve(root, "../../docs/releases", `${version}.md`)
  if (!existsSync(notes) || !readFileSync(notes, "utf8").trim()) throw new Error(`Missing release notes for ${version}. Run npm run release:version first.`)
  const head = run("git", ["rev-parse", "HEAD"])
  const localTag = run("git", ["tag", "--list", tag])
  if (localTag && run("git", ["rev-parse", `${tag}^{commit}`]) !== head) throw new Error(`${tag} belongs to a different commit. Check out that release or prepare a newer version.`)

  if (action === "draft") {
    preflight(root)
    // Fail here, not after an hour of CI builds: release only history that is on
    // origin/main, and only a version newer than every release tag on origin.
    run("git", ["fetch", "--quiet", "origin", "main"])
    try { run("git", ["merge-base", "--is-ancestor", "HEAD", "origin/main"]) }
    catch { throw new Error("This commit is not on origin/main. Merge the release preparation into main and release from that checkout. No tag was pushed.") }
    const remoteTags = new Map()
    for (const line of run("git", ["ls-remote", "--tags", "origin", "refs/tags/v*"]).split("\n")) {
      const match = /^(\S+)\s+refs\/tags\/v(\d+\.\d+\.\d+)(\^\{\})?$/.exec(line.trim())
      // Prefer the peeled commit of an annotated tag.
      if (match && (match[3] || !remoteTags.has(match[2]))) remoteTags.set(match[2], match[1])
    }
    const compare = (a, b) => {
      const [x, y] = [a, b].map(value => value.split(".").map(Number))
      return x[0] - y[0] || x[1] - y[1] || x[2] - y[2]
    }
    const newer = [...remoteTags.keys()].filter(other => compare(other, version) > 0).sort(compare).at(-1)
    if (newer) throw new Error(`origin already has v${newer}, newer than ${version}. Prepare a newer version. No tag was pushed.`)
    if (remoteTags.has(version) && remoteTags.get(version) !== head) throw new Error(`The remote ${tag} belongs to a different commit. Prepare a newer version. No tag was pushed.`)
    // The library creates the tag; push only this version, never unrelated local tags.
    if (!localTag) run(process.execPath, [resolve(root, "node_modules/@changesets/cli/bin.js"), "git-tag"])
    if (run("git", ["rev-parse", `${tag}^{commit}`]) !== head) throw new Error(`${tag} does not identify this commit.`)
    run("git", ["push", "origin", `refs/tags/${tag}`])
    console.log(`Pushed ${tag}. GitHub Actions builds a draft, Developer ID signed and notarized, or signed ad hoc when no Apple credentials are set and SILO_RELEASE_ALLOW_UNNOTARIZED is true; approve release-signing if requested. A repeated push does not restart a build. To retry, run Build Silo release on ${tag} with draft enabled.`)
  } else {
    // Dispatch against the immutable remote tag, not an unpushed local checkout.
    const remote = run("git", ["ls-remote", "origin", `refs/tags/${tag}`, `refs/tags/${tag}^{}`]).split("\n")
    const peeled = remote.find(line => line.endsWith(`refs/tags/${tag}^{}`)) ?? remote.find(line => line.endsWith(`refs/tags/${tag}`))
    if (peeled?.split(/\s+/)[0] !== head) throw new Error(`The remote ${tag} does not identify this commit. Publish from the tagged release checkout.`)
    run("gh", ["workflow", "run", "publish-release.yml", "--ref", tag, "-f", `version=${version}`, ...(allowStable ? ["-f", "allow_stable=true"] : [])])
    console.log(`Requested verified publication of ${tag}. Approve release-publish if requested; publication is complete only when the workflow succeeds.`)
  }
}

if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { release(process.argv[2], app, undefined, { allowStable: process.argv.includes("--allow-stable") }) }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
