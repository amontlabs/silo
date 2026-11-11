import { useEffect, useRef, useState } from "react"

import { OnboardingApp, type OnboardingAppProps } from "@/features/onboarding/onboarding-app"
import type { GitHubConnectionState, OnboardingActions } from "@/features/onboarding/model/onboarding-source"
import { onboardingScenarios } from "./scenarios"
import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { fixtureApplicationCatalog } from "./application-catalog"
import { useSettings } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "./system-integrations"
import { createAgentFixture, type AgentFixtureMode } from "./agents"
import type { OnboardingCompletionRequest } from "@/features/onboarding/model/onboarding-source"

interface OnboardingPreviewProps extends Omit<OnboardingAppProps, "actions" | "githubConnectionState" | "completed"> {
  actions?: Partial<OnboardingActions>
  initialGitHubConnectionState?: GitHubConnectionState
  initialCompleted?: boolean
  /** Installs the chosen agent the way the engine would, in this state. Without it the preview has no agent install. */
  agents?: AgentFixtureMode
}

export function OnboardingPreview({ source: initialSource, actions, initialGitHubConnectionState, initialCompleted = false, agents, ...props }: OnboardingPreviewProps) {
  const [source, setSource] = useState(initialSource)
  const [completed, setCompleted] = useState(initialCompleted)
  const [githubConnectionState, setGitHubConnectionState] = useState<GitHubConnectionState>(
    initialGitHubConnectionState ?? (source.githubPolicies.some(({ repositories }) => repositories.length > 0) ? "connected" : "disconnected"),
  )
  const connectTimer = useRef<number | undefined>(undefined)
  const { store } = useSettings()
  const [systemIntegrations] = useState(() => createFixtureSystemIntegrationStore(store))

  useEffect(() => () => window.clearTimeout(connectTimer.current), [])

  // The agent install of setup, walked through its steps when the preview has agents.
  const [agentFixture] = useState(() => agents ? createAgentFixture({ mode: agents }) : null)
  const installed = useRef<string | null>(null)
  const lastInstall = useRef<{ computer: string; agent: "claude-code" | "codex" } | null>(null)
  function installAgent(computer: string, agent: "claude-code" | "codex") {
    if (!agentFixture) return
    lastInstall.current = { computer, agent }
    setSource(current => ({ ...current, agentSetup: { computer, agent, status: "running", step: "waiting" } }))
    agentFixture.installAgent(computer, agent, progress => setSource(current => ({ ...current, agentSetup: { computer, agent, status: "running", step: progress.step, ...(progress.detail && { detail: progress.detail }) } }))).then(
      () => setSource(current => ({ ...current, agentSetup: { computer, agent, status: "succeeded" } })),
      (error: unknown) => setSource(current => ({ ...current, agentSetup: { computer, agent, status: "failed", failure: error instanceof Error ? error.message : String(error) } })),
    )
  }
  function queueAgent(request: OnboardingCompletionRequest) {
    const computer = request.computerConfiguration.computers[0]?.name
    if (!agentFixture || !computer || !request.agent || request.agent === "none") return
    const key = `${computer}:${request.agent}`
    if (installed.current === key) return
    installed.current = key
    installAgent(computer, request.agent)
  }

  return <ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><SystemIntegrationProvider store={systemIntegrations}><OnboardingApp
    {...props}
    source={{ ...source, preflightChecks: initialSource.preflightChecks }}
    completed={completed}
    presentationOnlyCompleted={initialCompleted}
    githubConnectionState={githubConnectionState}
    actions={{
      connectGitHub: () => {
        actions?.connectGitHub?.()
        window.clearTimeout(connectTimer.current)
        setGitHubConnectionState("connecting")
        connectTimer.current = window.setTimeout(() => setGitHubConnectionState("connected"), 700)
      },
      cancelGitHubConnection: () => {
        window.clearTimeout(connectTimer.current)
        setGitHubConnectionState("disconnected")
        actions?.cancelGitHubConnection?.()
      },
      reopenGitHubAuthorization: () => actions?.reopenGitHubAuthorization?.(),
      saveComputerConfiguration: (request) => {
        actions?.saveComputerConfiguration?.(request)
        setSource((current) => ({ ...current, computerConfigurations: request.computers }))
      },
      retryComputerSetup: () => {
        actions?.retryComputerSetup?.()
        setSource((current) => ({
          ...onboardingScenarios.running,
          preflightChecks: current.preflightChecks,
        }))
      },
      submitStep: (step, request, options) => {
        actions?.submitStep?.(step, request, options)
        if (step !== "computers") queueAgent(request)
      },
      ...(agentFixture && {
        retryAgentInstall: () => { if (lastInstall.current) installAgent(lastInstall.current.computer, lastInstall.current.agent) },
        openAgent: () => undefined,
        ensureSshName: agentFixture.ensureSshName,
      }),
      finishSetup: (request) => {
        actions?.finishSetup?.(request)
        queueAgent(request)
        setCompleted(true)
      },
    }}
  /></SystemIntegrationProvider></ApplicationCatalogProvider>
}
