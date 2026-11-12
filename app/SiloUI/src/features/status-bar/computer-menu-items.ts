import type { ApplicationSource, ApplicationComputer } from "@/features/application/model/application-source"
import { computerAvailability } from "@/features/application/model/computer-availability"
import { computerCan } from "@/features/computers/model/computer-capabilities"
import type { AgentId } from "@/features/agents/model/agents"

/**
 * One computer's "…" menu in the status panel, as data. The desktop app renders it as a
 * native menu (`desktop/native-computer-menu.tsx`); the browser preview renders the same
 * items with Radix (`computer-menu.tsx`), so tests of either exercise what users see.
 */
export type ComputerMenuItem =
  | { kind: "action"; id: string; label: string; enabled: boolean; run: () => void }
  | { kind: "separator" }
  | { kind: "submenu"; id: string; label: string; enabled: boolean; items: ComputerMenuItem[] }
  /** Copies `value`; renderers report the result with these labels. */
  | { kind: "copy"; id: string; label: string; value: string; copied: string; failed: string }

export interface ComputerMenuHandlers {
  start: () => void
  /** Stop and Restart always ask first (decision 8), so they only request a confirmation. */
  confirm: (action: "stop" | "restart") => void
  openTerminal: () => void
  /** The agents that can be opened in this computer, and how to open one. Absent when none can. */
  agents?: { id: AgentId; name: string }[]
  openAgent?: (agent: AgentId) => void
  /** Opens the folder picker for the configured editor. */
  chooseFolder: () => void
  openSite: (port: number) => void
}

/** Sites that `open_network_port` accepts: configured, reachable, with a scheme and host port. */
export function computerSites(computer: ApplicationComputer) {
  return computer.ports
    .filter(({ listening, configured, scheme, hostPort }) => listening === true && configured === true && scheme != null && hostPort != null)
    .sort((a, b) => a.port - b.port)
}

export function computerMenuItems(computer: ApplicationComputer, source: ApplicationSource, handlers: ComputerMenuHandlers): ComputerMenuItem[] {
  const { canOpen, canStart, canStop, canRestart } = computerAvailability(computer, source)
  const sites = computerSites(computer)
  return [
    ...(computer.state === "stopped"
      ? [{ kind: "action", id: "start", label: "Start", enabled: canStart, run: handlers.start } as const]
      : [
        { kind: "action", id: "stop", label: "Stop…", enabled: canStop, run: () => handlers.confirm("stop") } as const,
        { kind: "action", id: "restart", label: "Restart…", enabled: canRestart, run: () => handlers.confirm("restart") } as const,
      ]),
    { kind: "separator" },
    ...(computerCan(computer.configuration, "terminal") ? [{ kind: "action", id: "terminal", label: `Open in ${source.preferences.terminal}`, enabled: canOpen, run: handlers.openTerminal } as const] : []),
    ...(handlers.openAgent ? (handlers.agents ?? []).map(({ id, name }) => ({ kind: "action", id: `agent:${id}`, label: `Open ${name}`, enabled: canOpen, run: () => handlers.openAgent!(id) } as const)) : []),
    ...(computerCan(computer.configuration, "editor") ? [{ kind: "action", id: "editor", label: `Open in ${source.preferences.editor}…`, enabled: canOpen, run: handlers.chooseFolder } as const] : []),
    {
      kind: "submenu",
      id: "sites",
      label: "Open in browser",
      enabled: canOpen,
      items: sites.length
        ? [
          ...sites.map(({ port }) => ({ kind: "action", id: `site:${port}`, label: `Port ${port}`, enabled: true, run: () => handlers.openSite(port) } as const)),
          { kind: "separator" } as const,
          ...sites.map(({ port, scheme, hostPort, host }) => ({
            kind: "copy",
            id: `copy:${port}`,
            label: `Copy port ${port} address`,
            value: `${scheme}://${host ?? "127.0.0.1"}:${hostPort}`,
            copied: `Port ${port} address copied`,
            failed: `Could not copy port ${port} address`,
          } as const)),
        ]
        : [{ kind: "action", id: "no-sites", label: "No reachable ports", enabled: false, run: () => {} }],
    },
  ]
}
