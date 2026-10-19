import { useEffect, useLayoutEffect, useRef, useState } from "react"

import { bridgeErrorMessage } from "@/contracts/bridge-error"
import { errorMessage, showActionFailure, showOperationFailure, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"
import { computerTarget } from "@/features/application/model/connections"
import type { ApplicationActions, ApplicationComputer, NetworkPort, NetworkState } from "@/features/application/model/application-source"

/** Where a forwarded port is reached on this device. Websites use their computer's own
 * `*.localhost` name when the backend supplies one, so browsers keep each computer's cookies
 * apart from other local services in every browser; plain TCP ports use 127.0.0.1. */
export function networkAddress(port: NetworkPort, host?: string | null) {
  if (port.hostPort === null) return null
  return port.scheme ? `${port.scheme}://${host ?? "127.0.0.1"}:${port.hostPort}` : `127.0.0.1:${port.hostPort}`
}

/** The same port at 127.0.0.1, for development servers that reject unfamiliar host names. */
export function networkLoopbackAddress(port: NetworkPort) {
  return networkAddress(port, null)
}

/** The human-readable state of a port, accounting for computer lifecycle and stale/failed discovery. */
export function networkPortState(computer: ApplicationComputer, port: NetworkPort, error?: string | null) {
  if (computer.freshness === "stale") return "Unknown"
  if (computer.state === "starting") return computer.stateDetail === "Stopping" ? "Computer stopping" : "Computer starting"
  if (computer.state !== "running") return computer.state === "failed" ? "Computer failed" : "Computer stopped"
  if (error) return "Unknown"
  return ({ reachable: "Reachable", waiting: "Waiting for service", unpublished: "Not forwarded", unknown: "Unknown" })[port.state]
}

interface PortOperationIdentity { device?: { id: string; name: string }; computerId: string; displayName: string }

interface PortDraft { computer: string; computerId: string; port: string; hostPort: string; scheme: string; editing: boolean }

/** Shared state and operations for adding, editing, connecting, and removing forwarded ports.
 * Both the full Network page and a computer's Ports section drive identical behaviour from it. */
export function useNetworkPorts({ computers, network, error, actions, active }: {
  computers: ApplicationComputer[]
  network?: NetworkState
  error?: string | null
  actions: ApplicationActions
  active: boolean
}) {
  const [draft, setDraft] = useState<PortDraft | null>(null)
  const [fieldErrors, setFieldErrors] = useState<{ port?: string; hostPort?: string; computer?: string }>({})
  const [connecting, setConnecting] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const pending = useRef(false)
  const [confirm, setConfirm] = useState<string | null>(null)
  const refreshNetwork = actions.refreshNetwork
  const currentComputers = useRef<ApplicationComputer[] | null>(computers)
  useLayoutEffect(() => {
    currentComputers.current = computers
    return () => { currentComputers.current = null }
  }, [computers])

  function hasCurrentComputer(identity: PortOperationIdentity) {
    return currentComputers.current?.some(computer => computer.configuration.id === identity.computerId
      && computer.device?.id === identity.device?.id && computer.configuration.name === identity.displayName)
  }
  const changedComputer = "This computer changed or is no longer available. Open its current Ports section and try again."

  useEffect(() => {
    if (!active || !refreshNetwork) return
    const refresh = (background = false) => { if (document.visibilityState !== "hidden") void refreshNetwork({ background }) }
    // Focus and visibility changes arrive together when the window returns: one refresh covers both.
    let lastReturn = 0
    const onReturn = () => {
      if (Date.now() - lastReturn < 1000) return
      lastReturn = Date.now()
      refresh()
    }
    refresh()
    const timer = window.setInterval(() => refresh(true), 12_000)
    window.addEventListener("focus", onReturn)
    document.addEventListener("visibilitychange", onReturn)
    return () => { clearInterval(timer); window.removeEventListener("focus", onReturn); document.removeEventListener("visibilitychange", onReturn) }
  }, [active, refreshNetwork])

  /** Runs a port operation with the shared loading → success/failure notifications. Failures offer Retry. */
  async function run(id: string, identity: PortOperationIdentity, copy: { loading: string; step?: string; success: string; failure: string }, operation: () => Promise<void>, onSuccess?: () => void): Promise<boolean> {
    if (pending.current) return false
    if (!hasCurrentComputer(identity)) {
      showOperationFailure(id, copy.failure, { description: changedComputer, native: false })
      return false
    }
    pending.current = true
    setBusy(true)
    const computer = identity.displayName
    const location = identity.device ? `${computer} · ${identity.device.name}` : computer
    const noticeComputer = { id: identity.computerId, name: computer }
    showOperationProgress(id, { title: `${copy.loading} · ${location}`, step: `${copy.step ?? copy.loading} · ${location}`, progress: null, computer })
    try {
      await operation()
      showOperationSuccess(id, `${copy.success} · ${location}`, { computer, noticeComputer })
      setConfirm(null)
      onSuccess?.()
      return true
    } catch (cause) {
      const message = bridgeErrorMessage(cause) ?? (typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "The port could not be updated.")
      showOperationFailure(id, `${copy.failure} · ${location}`, { description: message, retry: () => void run(id, identity, copy, operation, onSuccess), computer, noticeComputer })
      return false
    } finally { pending.current = false; setBusy(false) }
  }

  /** Opening is instant, so it has no loading phase: a failure stays until closed, with Retry. */
  async function open(computer: ApplicationComputer, port: number) {
    const location = computer.device ? `${computer.configuration.name} · ${computer.device.name}` : computer.configuration.name
    const attempt = async () => {
      if (!hasCurrentComputer({ device: computer.device, computerId: computer.configuration.id, displayName: computer.configuration.name })) {
        showActionFailure(`Could not open port ${port} · ${location}`, changedComputer, undefined, { id: `network-port-open:${computer.configuration.id}:${port}`, native: false })
        return
      }
      try { await actions.openNetworkPort!(computerTarget(computer), port) }
      catch (cause) { showActionFailure(`Could not open port ${port} · ${location}`, typeof cause === "string" ? cause : errorMessage(cause), () => void attempt(), { id: `network-port-open:${computer.configuration.id}:${port}`, noticeComputer: { id: computer.configuration.id, name: computer.configuration.name } }) }
    }
    await attempt()
  }

  const localComputers = computers
  const loading = Boolean(refreshNetwork) && localComputers.some(computer => !network?.computers.some(item => item.computer === computerTarget(computer)))
  const rows = computers.flatMap(computer => {
    const item = network?.computers.find(item => item.computer === computerTarget(computer))
    return (item?.ports ?? []).map(port => ({ computer, port, host: item?.host ?? null, error: item?.error ?? (computer.device ? null : error) }))
  })
    .sort((a, b) => a.computer.configuration.name.localeCompare(b.computer.configuration.name) || a.port.port - b.port.port)
  const errors = computers.flatMap(computer => {
    const item = network?.computers.find(item => item.computer === computerTarget(computer))
    // A stopped computer has no live services to observe; its saved ports show as "Computer stopped".
    const ambiguous = computers.some(other => other !== computer && other.configuration.name === computer.configuration.name)
    const name = ambiguous ? `${computer.configuration.name} (${computer.device?.name ?? "This device"})` : computer.configuration.name
    return item?.error && computer.state !== "stopped" ? [`${name}: ${item.error}`] : []
  })

  const runningLocalComputers = localComputers.filter(computer => computer.state === "running")
  /** Why "Add port" is unavailable, or null when it can be used. */
  const addDisabledReason = runningLocalComputers.length > 0 ? null
    : localComputers.length === 1 ? `Start ${localComputers[0].configuration.name} to add ports`
    : localComputers.length > 1 ? "Start a computer to add ports" : null

  function add(computer = runningLocalComputers[0] ? computerTarget(runningLocalComputers[0]) : "", port = "") {
    setFieldErrors({}); setDraft({ computer, computerId: localComputers.find(item => computerTarget(item) === computer)?.configuration.id ?? "", port, hostPort: "", scheme: "http", editing: false })
  }
  function startEdit(computer: ApplicationComputer, port: NetworkPort) {
    setFieldErrors({})
    setDraft({ computer: computerTarget(computer), computerId: computer.configuration.id, port: String(port.port), hostPort: port.configuredHostPort == null ? "" : String(port.configuredHostPort), scheme: port.scheme ?? "tcp", editing: true })
  }
  function cancelDraft() { setDraft(null) }

  return {
    error: computers.some(computer => !computer.device) ? error : null, draft, setDraft, fieldErrors, setFieldErrors, connecting, setConnecting, busy,
    /** Always null: operation failures are shown as notifications, not rendered inline. */
    confirm, setConfirm,
    run, open, add, startEdit, cancelDraft, loading, localComputers, runningLocalComputers, addDisabledReason, rows, errors, actions,
  }
}

export type NetworkPortsController = ReturnType<typeof useNetworkPorts>
