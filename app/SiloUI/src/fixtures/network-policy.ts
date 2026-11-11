import type { ComputerTools, NetworkPolicy, NetworkPolicyState } from "@/desktop/production-schemas"
import type { ApplicationActions, ApplicationComputer } from "@/features/application/model/application-source"
import { computerTarget } from "@/features/application/model/connections"

/**
 * The network of each computer and Silo's tools in it, in the preview. `mixed` gives the first
 * computers an open, an allowlist and a closed network and puts the tools on in the first.
 * The other modes put every computer in one state: `update` has a preset with an update to
 * accept and a notice, `stale-save` refuses the first save because the policy changed, and the
 * `tools-` modes show the tools on, off, waiting for a restart or with a piece that failed.
 */
export const networkFixtureModes = ["mixed", "open", "allowlist", "closed", "update", "stale-save", "tools-on", "tools-off", "tools-restart", "tools-failed"] as const
export type NetworkFixtureMode = (typeof networkFixtureModes)[number]

export function networkFixtureModeFromSearch(search: string): NetworkFixtureMode | undefined {
  const requested = new URLSearchParams(search).get("network-policy")
  return networkFixtureModes.find(mode => mode === requested)
}

export const fixturePresets = [
  { name: "npm", title: "npm packages", hosts: ["registry.npmjs.org"] },
  { name: "pypi", title: "Python packages", hosts: ["pypi.org", "files.pythonhosted.org"] },
  { name: "anthropic", title: "Anthropic API", hosts: ["api.anthropic.com"] },
]

const open: NetworkPolicy = { mode: "open", hosts: [], presets: [] }
const closed: NetworkPolicy = { mode: "closed", hosts: [], presets: [] }
const allowlist: NetworkPolicy = { mode: "allowlist", hosts: ["example.com", "*.internal.example.org"], presets: ["npm", "pypi"] }

const secretNotice = "The secret stripe-key allows *.stripe.com, which is not used as a network rule. Only api.stripe.com is reachable."

function stateFor(computerId: string, policy: NetworkPolicy, extra: Partial<NetworkPolicyState> = {}): NetworkPolicyState {
  const listed = policy.mode !== "allowlist" ? [] : [
    ...policy.hosts.map(host => ({ host, ports: [443], from: "list" })),
    ...policy.presets.flatMap(name => (fixturePresets.find(preset => preset.name === name)?.hosts ?? []).map(host => ({ host, ports: [443], from: `preset:${name}` }))),
    { host: "github.com", ports: [443], from: "github" },
    { host: "api.stripe.com", ports: [443], from: "secret:stripe-key" },
  ]
  return { computerId, policy, allowedHosts: listed, timing: "applied", presets: fixturePresets, presetUpdates: [], notices: [], ...extra }
}

export type NetworkFixtureActions = Required<Pick<ApplicationActions, "readNetworkPolicy" | "saveNetworkPolicy" | "readComputerTools" | "setComputerTools">>

/** Network policies and tools that answer from memory; a save changes what the next read returns. */
export function createNetworkFixture({ mode = "mixed" }: { mode?: NetworkFixtureMode } = {}): NetworkFixtureActions & { setComputers: (computers: readonly ApplicationComputer[]) => void } {
  let computers: readonly ApplicationComputer[] = []
  const policies = new Map<string, NetworkPolicyState>()
  const tools = new Map<string, ComputerTools>()
  let refused = mode === "stale-save"
  const index = (target: string) => Math.max(0, computers.findIndex(computer => computerTarget(computer) === target))
  const running = (target: string) => computers.find(computer => computerTarget(computer) === target)?.state === "running"

  const initialPolicy = (target: string): NetworkPolicyState => {
    const id = target
    switch (mode) {
      case "open": case "tools-on": case "tools-off": case "tools-restart": case "tools-failed": return stateFor(id, open)
      case "closed": return stateFor(id, closed)
      case "allowlist": case "stale-save": return stateFor(id, allowlist, { notices: [secretNotice] })
      case "update": return stateFor(id, allowlist, {
        presetUpdates: [{ name: "npm", title: "npm packages", addedHosts: ["registry.npmmirror.com", "npm.pkg.example"] }],
        notices: [secretNotice],
      })
      case "mixed": return [stateFor(id, open), stateFor(id, allowlist), stateFor(id, closed)][index(target) % 3]!
    }
  }
  const initialTools = (target: string): ComputerTools => {
    const base = { computerId: target, restartNeeded: false, components: [] }
    switch (mode) {
      case "tools-on": return { ...base, enabled: true, network: "applied" }
      case "tools-restart": return { ...base, enabled: true, network: "nextRestart", restartNeeded: true }
      case "tools-failed": return { ...base, enabled: true, network: "applied", components: [
        { id: "silo", state: "ok", detail: undefined },
        { id: "silo-mcp", state: "failed", detail: "Claude Code could not be reached." },
      ] }
      case "mixed": return { ...base, enabled: index(target) === 0, network: "applied" }
      default: return { ...base, enabled: false }
    }
  }
  const policyOf = (target: string) => policies.get(target) ?? initialPolicy(target)
  const toolsOf = (target: string) => tools.get(target) ?? initialTools(target)

  return {
    setComputers: next => { computers = next },
    readNetworkPolicy: async target => policyOf(target),
    saveNetworkPolicy: async (target, policy, expected) => {
      const current = policyOf(target)
      if (refused || (expected && JSON.stringify(expected) !== JSON.stringify(current.policy))) {
        refused = false
        throw Object.assign(new Error("The network policy of this computer changed while this was being decided. Read it again and retry."), { code: "changed" })
      }
      if (policy.mode === "allowlist") {
        const bad = policy.hosts.find(host => host !== host.toLowerCase() || /[:/\s]/.test(host))
        if (bad) throw new Error(`"${bad}" is not a host name. Use a lowercase name like example.com or *.example.com, without a port or an address.`)
      }
      const next = stateFor(target, policy, { timing: running(target) ? "nextRestart" : "nextStart", notices: current.notices })
      policies.set(target, next)
      return next
    },
    readComputerTools: async target => toolsOf(target),
    setComputerTools: async (target, enabled) => {
      const next: ComputerTools = enabled
        ? { computerId: target, enabled: true, network: running(target) ? "nextRestart" : "nextStart", restartNeeded: false, components: [] }
        : { computerId: target, enabled: false, restartNeeded: false, components: [] }
      tools.set(target, next)
      return next
    },
  }
}
