#!/usr/bin/env node
// One-time, explicit copy of production Silo's configuration into the Silo Dev
// channel. See docs/SiloUI-BUILD-CHANNELS.md. Production is only ever read.
//
//   npm --prefix app/SiloUI run dev:import-production-settings -- [--dry-run] [--yes]
//                                                                 [--include-github-oauth]
import { spawnSync } from "node:child_process"
import { randomUUID } from "node:crypto"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import readline from "node:readline/promises"
import { fileURLToPath } from "node:url"

import { DEVELOPMENT, PRODUCTION } from "./channel-names.mjs"
export { DEVELOPMENT, PRODUCTION }

/** The record of Silo's one-time saved-data vocabulary conversion, in the app data folder. */
export const VOCABULARY_RECORD = "vocabulary-migration.json"

/** Preferences that describe the person, not the computers or the OS registration. */
export const COPIED_SETTINGS = Object.freeze([
  "theme", "onboardingComplete", "reduceMotion",
  "notificationsEnabled", "notifyHealth", "notifyActions", "notifyBackup",
  "notifyFailures", "notifyChanges", "notifyCompletions", "notifyApprovals",
  "terminal", "terminalUseSystemDefault", "terminalPath",
  "editor", "editorUseSystemDefault", "editorPath",
  "browser", "browserUseSystemDefault", "browserPath",
])

export const HELP = `Copy production Silo's configuration into ${DEVELOPMENT.productName} (${DEVELOPMENT.identifier}), once.

Usage: npm --prefix app/SiloUI run dev:import-production-settings -- [options]

Options:
  --dry-run               Show what would be copied; change nothing.
  --yes                   Overwrite existing ${DEVELOPMENT.productName} configuration without asking.
  --include-github-oauth  Also duplicate the GitHub OAuth login. Off by default: GitHub
                          rotates refresh tokens, so the first channel to refresh logs
                          the other one out. The personal access token is always copied.
  --help                  Show this text.

Copied: app preferences and onboarding completion (theme, notifications, terminal,
editor and browser choices), the GitHub personal access token (Keychain duplicate),
secret definitions and values (Keychain duplicate) with no computer assignments, and
the list of connected devices with this device's client SSH key that reaches them.

Production may still use the saved-data names from before the computer vocabulary
(workspaces, machines, hosts) or already use the current ones; both are read. What is
written is always in the current names. Because ${DEVELOPMENT.productName} converts its saved data once
per channel, the import also removes ${DEVELOPMENT.productName}'s ${VOCABULARY_RECORD} so that conversion
runs again at the next launch and finishes whatever older files ${DEVELOPMENT.productName} still has.

${DEVELOPMENT.productName} always keeps its OWN device identity: a new device id is generated
(an existing one is preserved) and connections start switched off. Devices
you manage will see ${DEVELOPMENT.productName} as a separate device. The copied client key still
reaches the production Silo on each connected device, because that is what its
authorized_keys entry runs there.

Never copied: computers, checkpoints, disks, backups and their history, the
MicroSandbox home and runtime, per-computer network and SSH settings, launch-at-login
and startup-computer choices, update preferences, and anything under ~/.ssh.

Production is only read. The command refuses to run while ${DEVELOPMENT.productName} is running, and
asks before replacing anything ${DEVELOPMENT.productName} already has (previous files are kept as
*.bak-<time>). Secret values are never printed. On macOS, the Keychain may ask you to
allow access to the production items; on Linux, secret-tool is required.
`

export function channelPaths(channel, { home, platform = process.platform, env = process.env }) {
  if (platform === "darwin") {
    const support = path.join(home, "Library", "Application Support", channel.identifier)
    return { config: support, data: support, state: path.join(home, channel.stateDir) }
  }
  const base = (name, fallback) => (env[name] && path.isAbsolute(env[name]) ? env[name] : path.join(home, fallback))
  return {
    config: path.join(base("XDG_CONFIG_HOME", ".config"), channel.identifier),
    data: path.join(base("XDG_DATA_HOME", ".local/share"), channel.identifier),
    state: path.join(home, channel.stateDir),
  }
}

/** macOS `security` and Linux `secret-tool`. Values travel only through pipes and files. */
export function systemKeychain({ platform = process.platform, run = spawnSync } = {}) {
  if (platform === "darwin") {
    return {
      read(service, account) {
        const result = run("security", ["find-generic-password", "-s", service, "-a", account, "-w"], { encoding: "utf8" })
        if (result.status === 44) return null
        if (result.status !== 0) throw new Error(`Could not read ${service}/${account} from the Keychain.`)
        return result.stdout.replace(/\n$/, "")
      },
      write(service, account, value) {
        // `security -i` reads the command from stdin, so the value is not in any argument list.
        const hex = Buffer.from(value, "utf8").toString("hex")
        const result = run("security", ["-i"], {
          encoding: "utf8",
          input: `add-generic-password -U -s ${service} -a ${account} -l ${service} -X ${hex}\n`,
        })
        if (result.status !== 0) throw new Error(`Could not write ${service}/${account} to the Keychain.`)
      },
    }
  }
  return {
    read(service, account) {
      const result = run("secret-tool", ["lookup", "service", service, "username", account], { encoding: "utf8" })
      if (result.error) throw new Error("secret-tool is required to copy credentials on Linux.")
      if (result.status === 1 && result.stdout === "") return null
      if (result.status !== 0) throw new Error(`Could not read ${service}/${account} from the secret service.`)
      return result.stdout
    },
    write(service, account, value) {
      const result = run("secret-tool", ["store", `--label=${service}`, "service", service, "username", account, "application", "rust-keyring"], {
        encoding: "utf8",
        input: value,
      })
      if (result.status !== 0) throw new Error(`Could not write ${service}/${account} to the secret service.`)
    },
  }
}

/** True when `ps` output lists a development-channel process (bundle or debug binary). */
export function devProcessRunning(psOutput) {
  return psOutput.split("\n").some(line => line.includes(`${DEVELOPMENT.productName}.app/Contents/MacOS/`)
    || /\/debug\/silo-ui(\s|$)/.test(line)
    || new RegExp(`${RegExp.escape(DEVELOPMENT.remoteBridge)}(\\s|$)`).test(line))
}

export function systemDevRunning({ run = spawnSync } = {}) {
  const ps = run("ps", ["-axo", "command="], { encoding: "utf8" })
  if (ps.error || ps.status !== 0) throw new Error(`Could not list processes to check that ${DEVELOPMENT.productName} is not running.`)
  return devProcessRunning(ps.stdout ?? "")
}

function readJson(file) {
  try {
    return { value: JSON.parse(fs.readFileSync(file, "utf8")) }
  } catch (error) {
    return error.code === "ENOENT" ? { missing: true } : { invalid: true }
  }
}

const isObject = value => value !== null && typeof value === "object" && !Array.isArray(value)

export function sanitizeSettings(document) {
  const settings = {}
  for (const key of COPIED_SETTINGS) if (isObject(document.settings) && key in document.settings) settings[key] = document.settings[key]
  return { schemaVersion: document.schemaVersion ?? 1, settings, onboardingDraft: null }
}

/** Definitions only: no computer assignments, pending work, errors, or activity. */
export function sanitizeSecrets(document) {
  const secrets = (Array.isArray(document.secrets) ? document.secrets : [])
    .filter(secret => isObject(secret) && !secret.removing && typeof secret.valueId === "string")
    .map(secret => ({
      id: secret.id,
      name: secret.name,
      valueId: secret.valueId,
      computers: [],
      allowedDomains: Array.isArray(secret.allowedDomains) ? secret.allowedDomains : [],
      affected: [],
      pendingComputers: [],
      errors: {},
      removing: false,
    }))
  return { pendingRevocations: [], secrets, activities: [] }
}

function sanitizeGithubDocument(document) {
  return { revision: 0, accessEnabled: Boolean(document.accessEnabled), account: typeof document.account === "string" ? document.account : null }
}

/** The connected devices of a remote config in either vocabulary; the earlier `hosts` wins over `devices`, as in the saved-data conversion. */
function validDevices(config) {
  const list = "hosts" in config ? config.hosts : config.devices
  return (Array.isArray(list) ? list : []).filter(host =>
    isObject(host) && ["id", "name", "address"].every(key => typeof host[key] === "string" && host[key] !== ""))
}

function writeAtomic(file, bytes, mode) {
  const parent = path.dirname(file)
  fs.mkdirSync(parent, { recursive: true, mode: 0o700 })
  const temporary = path.join(parent, `.import-${process.pid}-${randomUUID()}`)
  const output = fs.openSync(temporary, "wx", mode)
  try {
    try {
      fs.writeFileSync(output, bytes)
      fs.fchmodSync(output, mode)
      fs.fsyncSync(output)
    } finally {
      fs.closeSync(output)
    }
    fs.renameSync(temporary, file)
    const directory = fs.openSync(parent, "r")
    try {
      fs.fsyncSync(directory)
    } finally {
      fs.closeSync(directory)
    }
  } finally {
    fs.rmSync(temporary, { force: true })
  }
}

function backup(file, stamp, mode) {
  if (fs.existsSync(file)) writeAtomic(`${file}.bak-${stamp}`, fs.readFileSync(file), mode)
}

/**
 * Plans, confirms, then performs the copy. Everything is injectable so tests use a
 * temporary HOME, an in-memory keychain and no real processes.
 */
export async function importProductionSettings({
  home,
  platform = process.platform,
  env = process.env,
  keychain,
  isDevRunning,
  confirm = async () => false,
  yes = false,
  dryRun = false,
  includeGithubOauth = false,
  log = () => {},
  now = () => new Date(),
  newId = randomUUID,
} = {}) {
  if (!home || !path.isAbsolute(home)) throw new Error("A home directory is required.")
  keychain ??= systemKeychain({ platform })
  isDevRunning ??= () => systemDevRunning()
  if (isDevRunning()) throw new Error(`${DEVELOPMENT.productName} is running. Quit it, then run this command again.`)

  const source = channelPaths(PRODUCTION, { home, platform, env })
  const target = channelPaths(DEVELOPMENT, { home, platform, env })
  const stamp = now().toISOString().replace(/[:.]/g, "-")
  const warnings = []
  const copied = []
  const skipped = []
  /** @type {{label: string, overwrites: boolean, file?: string, apply: () => void}[]} */
  const actions = []

  const devFile = file => {
    const root = [target.config, target.data, target.state].find(root => file.startsWith(root + path.sep))
    const inProduction = [source.config, source.data, source.state].some(root => file.startsWith(root + path.sep))
    if (!root || inProduction) throw new Error(`Refusing to write outside the ${DEVELOPMENT.productName} channel: ${file}`)
    const components = []
    for (let current = file; current !== path.dirname(root); current = path.dirname(current)) components.unshift(current)
    for (const current of components) {
      if (fs.lstatSync(current, { throwIfNoEntry: false })?.isSymbolicLink()) {
        throw new Error(`Refusing to write through a linked ${DEVELOPMENT.productName} destination: ${current}`)
      }
    }
    return file
  }
  const devKeychain = {
    read: (service, account) => keychain.read(service, account),
    write(service, account, value) {
      if (!Object.values(DEVELOPMENT.keychain).includes(service)) throw new Error(`Refusing to write to ${service}.`)
      keychain.write(service, account, value)
    },
  }
  const addFile = (label, file, bytes, mode = 0o600) => {
    const destination = devFile(file)
    actions.push({
      label,
      file: destination,
      overwrites: fs.existsSync(destination),
      apply() {
        backup(destination, stamp, mode)
        writeAtomic(destination, bytes, mode)
      },
    })
  }
  const addKeychain = (label, service, account, value) => {
    actions.push({
      label,
      overwrites: devKeychain.read(DEVELOPMENT.keychain[service], account) !== null,
      apply() {
        devKeychain.write(DEVELOPMENT.keychain[service], account, value)
        if (devKeychain.read(DEVELOPMENT.keychain[service], account) !== value) {
          throw new Error(`The copy of ${label} did not verify.`)
        }
      },
    })
  }

  // Preferences, onboarding completion, notifications, terminal/editor/browser choices.
  const settings = readJson(path.join(source.config, "settings.json"))
  if (settings.value && isObject(settings.value)) {
    addFile("App preferences and onboarding completion", path.join(target.config, "settings.json"),
      `${JSON.stringify(sanitizeSettings(settings.value), null, 2)}\n`)
  } else skipped.push(settings.invalid ? "App preferences (production file is unreadable)" : "App preferences (none saved)")

  // Secrets: definitions without assignments, values duplicated under the dev service.
  const secrets = readJson(path.join(source.data, "secrets.json"))
  if (secrets.value && isObject(secrets.value)) {
    const definitions = sanitizeSecrets(secrets.value)
    if (definitions.secrets.length > 0) {
      addFile(`Secret definitions (${definitions.secrets.length}, no computer assignments)`,
        path.join(target.data, "secrets.json"), `${JSON.stringify(definitions, null, 2)}\n`)
      const raw = keychain.read(PRODUCTION.keychain.secrets, "values")
      let vault = null
      try { vault = raw === null ? null : JSON.parse(raw) } catch { vault = null }
      if (isObject(vault)) {
        const wanted = new Set(definitions.secrets.map(secret => secret.valueId))
        const filtered = Object.fromEntries(Object.entries(vault).filter(([id]) => wanted.has(id)))
        addKeychain("Secret values (Keychain)", "secrets", "values", JSON.stringify(filtered))
      } else warnings.push(`Secret definitions were copied but production's secret values could not be read; re-enter the values in ${DEVELOPMENT.productName}.`)
    } else skipped.push("Secrets (none defined)")
  } else skipped.push("Secrets (none defined)")

  // GitHub: the personal access token is static. The OAuth login rotates its refresh token.
  const token = keychain.read(PRODUCTION.keychain.github, "personal-token")
  if (token !== null) addKeychain("GitHub personal access token (Keychain)", "github", "personal-token", token)
  else skipped.push("GitHub personal access token (none saved)")
  if (includeGithubOauth) {
    const account = keychain.read(PRODUCTION.keychain.github, "account")
    if (account !== null) {
      addKeychain("GitHub OAuth login (Keychain)", "github", "account", account)
      const github = readJson(path.join(source.data, "github.json"))
      if (github.value && isObject(github.value)) {
        addFile("GitHub connection state (no repositories or computer grants)", path.join(target.data, "github.json"),
          `${JSON.stringify(sanitizeGithubDocument(github.value), null, 2)}\n`)
      }
      warnings.push("The GitHub OAuth login now exists in both channels. When either refreshes it, GitHub invalidates the other's copy; reconnect there if it happens.")
    } else skipped.push("GitHub OAuth login (none saved)")
  } else if (keychain.read(PRODUCTION.keychain.github, "account") !== null) {
    skipped.push(`GitHub OAuth login (connect GitHub in ${DEVELOPMENT.productName}, or pass --include-github-oauth)`)
  }

  // Connected devices and the client key. Dev keeps its own identity.
  const remoteSource = path.join(source.state, "desktop-remote")
  const remoteTarget = path.join(target.state, "desktop-remote")
  const remote = readJson(path.join(remoteSource, "config.json"))
  let remoteNames = []
  if (remote.value && isObject(remote.value)) {
    const devices = validDevices(remote.value)
    remoteNames = devices.map(device => device.name)
    const existing = readJson(path.join(remoteTarget, "config.json"))
    const own = existing.value && isObject(existing.value) ? existing.value : {}
    const ownId = [own.hostId, own.deviceId].find(id => typeof id === "string" && id !== "")
    const deviceId = ownId ?? newId()
    if (devices.length > 0) {
      addFile(`Connected devices (${devices.length}); ${DEVELOPMENT.productName} keeps its own device id and connections stay off`,
        path.join(remoteTarget, "config.json"),
        `${JSON.stringify({ deviceId, enabled: own.enabled === true, devices }, null, 2)}\n`)
      for (const name of ["id_ed25519", "id_ed25519.pub"]) {
        const key = path.join(remoteSource, name)
        if (fs.existsSync(key)) addFile(`Remote-management client key ${name}`, path.join(remoteTarget, name), fs.readFileSync(key), name.endsWith(".pub") ? 0o644 : 0o600)
        else if (name === "id_ed25519") warnings.push(`Production has no client SSH key yet; ${DEVELOPMENT.productName} will create its own and ask you to install it on each remote computer.`)
      }
    } else skipped.push("Connected devices (none saved)")
  } else skipped.push("Connected devices (none saved)")

  // Dev converts its saved data once per channel. Written files use the current names, and
  // clearing the record makes the conversion run again to finish any older Dev files.
  const vocabularyRecord = devFile(path.join(target.data, VOCABULARY_RECORD))
  if (actions.length > 0 && fs.existsSync(vocabularyRecord)) {
    actions.unshift({
      label: `Saved-data conversion record (${DEVELOPMENT.productName} runs the conversion again at its next launch)`,
      file: vocabularyRecord,
      overwrites: false,
      apply() {
        fs.rmSync(vocabularyRecord, { force: true })
        const directory = fs.openSync(path.dirname(vocabularyRecord), "r")
        try {
          fs.fsyncSync(directory)
        } finally {
          fs.closeSync(directory)
        }
      },
    })
  }

  if (actions.length === 0) {
    log("Nothing to import: production has no saved configuration to copy.")
    return { copied, skipped, warnings, performed: false }
  }
  log(`${DEVELOPMENT.productName} will receive:`)
  for (const action of actions) log(`  - ${action.label}${action.overwrites ? "  [replaces existing]" : ""}`)
  for (const name of remoteNames) log(`      remote computer: ${name}`)
  for (const line of skipped) log(`Not copied: ${line}`)
  for (const line of warnings) log(`Note: ${line}`)
  if (dryRun) {
    log("Dry run: nothing was changed.")
    return { copied: actions.map(action => action.label), skipped, warnings, performed: false }
  }
  const overwrites = actions.filter(action => action.overwrites)
  if (overwrites.length > 0 && !yes) {
    if (!(await confirm(`${DEVELOPMENT.productName} already has ${overwrites.length} of these. Replace them?`))) {
      log("Cancelled. Nothing was changed.")
      return { copied, skipped, warnings, performed: false }
    }
  }
  // Confirmation can remain open while destination entries change.
  if (isDevRunning()) throw new Error(`${DEVELOPMENT.productName} is running. Quit it, then run this command again.`)
  for (const action of actions) if (action.file) devFile(action.file)
  for (const action of actions) {
    action.apply()
    copied.push(action.label)
  }
  log(`Copied ${copied.length} item${copied.length === 1 ? "" : "s"} into ${DEVELOPMENT.productName}. Production was not modified.`)
  return { copied, skipped, warnings, performed: true }
}

async function main(argv) {
  const flags = new Set(argv)
  const unknown = argv.filter(arg => !["--dry-run", "--yes", "--include-github-oauth", "--help"].includes(arg))
  if (flags.has("--help")) return console.log(HELP)
  if (unknown.length > 0) {
    console.error(`Unknown option: ${unknown.join(" ")}\n\n${HELP}`)
    process.exitCode = 2
    return
  }
  const confirm = async question => {
    if (!process.stdin.isTTY) return false
    const prompt = readline.createInterface({ input: process.stdin, output: process.stdout })
    const answer = await prompt.question(`${question} [y/N] `)
    prompt.close()
    return /^y(es)?$/i.test(answer.trim())
  }
  try {
    await importProductionSettings({
      home: os.homedir(),
      yes: flags.has("--yes"),
      dryRun: flags.has("--dry-run"),
      includeGithubOauth: flags.has("--include-github-oauth"),
      confirm,
      log: line => console.log(line),
    })
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) await main(process.argv.slice(2))
