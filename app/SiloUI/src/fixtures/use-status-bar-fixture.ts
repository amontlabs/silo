import { useCallback, useEffect, useMemo, useRef, useState } from "react"

import type { ApplicationSource, RepositoryPushOperation } from "@/features/application/model/application-source"
import { computerAvailability } from "@/features/application/model/computer-availability"
import type { StatusBarActions, StatusBarRoute } from "@/features/status-bar/status-bar-types"
import { statusBarSourceForFixture, type StatusBarFixtureMode } from "./status-bar-scenarios"
import { useSettings } from "@/features/preferences/settings-store"

import { fixtureDirectoryLoader } from "./directory-loader"
import { agentFixtureModeFromSearch, createAgentFixture } from "./agents"

interface PreviewOperation {
  name: string
  state: "running" | "stopped"
  action: "started" | "stopped" | "restarted"
  title: string
  occurredAt: string
}

function completeOperation(source: ApplicationSource, operation: PreviewOperation): ApplicationSource {
  return {
    ...source,
    computers: source.computers.map((computer) => computer.configuration.name === operation.name ? {
      ...computer,
      state: operation.state,
      stateDetail: operation.state === "running" ? "Running" : "Stopped",
      attention: undefined,
      freshness: "fresh",
      ports: computer.ports.map((port, index) => ({ ...port, listening: operation.state === "running" && index === 0 })),
    } : computer),
  }
}

function completePush(source: ApplicationSource, operation: RepositoryPushOperation): ApplicationSource {
  return {
    ...source,
    computers: source.computers.map((computer) => computer.configuration.name === operation.computer ? {
      ...computer,
      repositories: computer.repositories.map((repository) => repository.path === operation.repositoryPath ? {
        ...repository,
        ahead: Math.max(0, repository.ahead - operation.commitCount),
      } : repository),
    } : computer),
    repositoryPushOperations: source.repositoryPushOperations.map((current) => current.computer === operation.computer && current.repositoryPath === operation.repositoryPath
      ? { ...operation, status: "succeeded" }
      : current),
  }
}

export function useStatusBarFixture(source: ApplicationSource, mode: StatusBarFixtureMode | undefined, onOpenSilo: (source: ApplicationSource, route?: StatusBarRoute) => void) {
  const { settings } = useSettings(source.preferences)
  const [settledSnapshot, setSnapshot] = useState(() => statusBarSourceForFixture(source, mode))
  const [pendingOperations, setPendingOperations] = useState<PreviewOperation[]>([])
  const operationTimers = useRef(new Map<string, number>())
  const pendingPushes = useRef(new Map<string, { operation: RepositoryPushOperation; timer: number }>())
  const [launched, setLaunched] = useState(true)
  const [acknowledgement, setAcknowledgement] = useState("")
  const snapshot: ApplicationSource = {
    ...settledSnapshot,
    preferences: { ...settledSnapshot.preferences, ...settings },
    computers: settledSnapshot.computers.map((computer) => {
      const pending = pendingOperations.find(({ name }) => name === computer.configuration.name)
      return pending ? { ...computer, state: pending.action === "stopped" ? computer.state : "starting", stateDetail: pending.title } : computer
    }),
    activities: [
      ...pendingOperations.map((operation) => ({
        id: `preview-${operation.name}`,
        computer: operation.name,
        category: "computer" as const,
        status: "running" as const,
        tone: "neutral" as const,
        title: operation.title,
        detail: "",
        occurredAt: operation.occurredAt,
        time: "Now",
      })),
      ...source.activities,
    ],
  }

  useEffect(() => {
    const timers = operationTimers.current
    const pushes = pendingPushes.current
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer))
      timers.clear()
      pushes.forEach(({ timer }) => window.clearTimeout(timer))
      pushes.clear()
    }
  }, [])

  function updateComputer(name: string, state: PreviewOperation["state"], action: PreviewOperation["action"]) {
    if (operationTimers.current.has(name)) return
    const title = `${action === "started" ? "Starting" : action === "stopped" ? "Stopping" : "Restarting"} ${name}…`
    const operation = { name, state, action, title, occurredAt: new Date().toISOString() }
    setPendingOperations((current) => [...current, operation])
    setAcknowledgement("")
    operationTimers.current.set(name, window.setTimeout(() => {
      setSnapshot((current) => completeOperation(current, operation))
      setPendingOperations((current) => current.filter((pending) => pending.name !== name))
      operationTimers.current.delete(name)
      setAcknowledgement(`Preview: ${name} ${action}.`)
    }, 900))
  }

  function settleOperations() {
    let next = pendingOperations.reduce(completeOperation, { ...settledSnapshot, preferences: { ...settledSnapshot.preferences, ...settings }, activities: source.activities })
    pendingPushes.current.forEach(({ operation, timer }) => {
      window.clearTimeout(timer)
      next = completePush(next, operation)
    })
    pendingPushes.current.clear()
    operationTimers.current.forEach((timer) => window.clearTimeout(timer))
    operationTimers.current.clear()
    setPendingOperations([])
    setSnapshot(next)
    return next
  }

  function pushRepository(name: string, repositoryPath: string) {
    const key = JSON.stringify([name, repositoryPath])
    if (pendingPushes.current.has(key)) return
    const computer = snapshot.computers.find(({ configuration }) => configuration.name === name)
    const repository = computer?.repositories.find(({ path }) => path === repositoryPath)
    if (!computer || !repository || repository.ahead <= 0 || !computerAvailability(computer, snapshot).canOpen) return
    if (snapshot.repositoryPushOperations.some((operation) => operation.computer === name && operation.repositoryPath === repositoryPath && operation.status === "pushing")) return
    const operation: RepositoryPushOperation = { computer: name, repositoryPath, commitCount: repository.ahead, status: "pushing" }
    setSnapshot((current) => ({
      ...current,
      repositoryPushOperations: [
        ...current.repositoryPushOperations.filter((pending) => pending.computer !== name || pending.repositoryPath !== repositoryPath),
        operation,
      ],
    }))
    const timer = window.setTimeout(() => {
      setSnapshot((current) => completePush(current, operation))
      pendingPushes.current.delete(key)
    }, 900)
    pendingPushes.current.set(key, { operation, timer })
  }

  const dismissRepositoryPush = useCallback((computer: string, repositoryPath: string) => {
    setSnapshot((current) => ({
      ...current,
      repositoryPushOperations: current.repositoryPushOperations.filter((operation) => operation.computer !== computer || operation.repositoryPath !== repositoryPath || operation.status !== "succeeded"),
    }))
  }, [])

  const listComputerDirectory = useMemo(() => fixtureDirectoryLoader(snapshot.computers), [snapshot.computers])
  const [agentFixture] = useState(() => createAgentFixture({ mode: agentFixtureModeFromSearch(window.location.search) }))
  useEffect(() => agentFixture.setComputers(snapshot.computers), [agentFixture, snapshot.computers])
  const actions: StatusBarActions = {
    listComputerDirectory,
    openSilo: (route) => onOpenSilo(settleOperations(), route),
    quit: () => {
      settleOperations()
      setLaunched(false)
      setAcknowledgement("Silo quit in this preview.")
    },
    refresh: () => {
      setSnapshot((current) => ({ ...current, computers: current.computers.map((computer) => ({ ...computer, freshness: "fresh" })) }))
      setAcknowledgement("Preview: computer status refreshed.")
    },
    startComputer: (name) => updateComputer(name, "running", "started"),
    stopComputer: (name) => updateComputer(name, "stopped", "stopped"),
    restartComputer: (name) => updateComputer(name, "running", "restarted"),
    openTerminal: (name) => setAcknowledgement(`Preview: open ${snapshot.preferences.terminal} in ${name}.`),
    openAgent: (name, agent) => setAcknowledgement(`Preview: open ${agent === "codex" ? "Codex" : "Claude Code"} in ${name}.`),
    listAgents: agentFixture.listAgents,
    openEditor: (name, path) => setAcknowledgement(`Preview: open ${path} in ${snapshot.preferences.editor} on ${name}.`),
    openSite: (name, port) => setAcknowledgement(`Preview: open ${name} port ${port} in ${snapshot.preferences.browser}.`),
    pushRepository,
    dismissRepositoryPush,
  }

  return { source: snapshot, actions, launched, acknowledgement, relaunch: () => { setLaunched(true); setAcknowledgement("") } }
}
