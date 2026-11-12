import { act, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { ApplicationCatalogProvider } from "@/features/preferences/application-catalog"
import { createMemorySettingsStore, SettingsProvider, type SettingsStore } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { OnboardingApp } from "@/features/onboarding/onboarding-app"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import type { OnboardingActions, OnboardingSource } from "@/features/onboarding/model/onboarding-source"
import { fixtureApplicationCatalog } from "@/fixtures/application-catalog"
import { onboardingScenarios } from "@/fixtures/scenarios"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"

const [real] = onboardingScenarios.complete.computerConfigurations
const onComputers = { currentStep: "computers" as const, computers: [real], unfinishedComputerEditor: null, computerSelections: {}, computerIdentities: {} }

function actions(extra: Partial<OnboardingActions> = {}): OnboardingActions {
  return { connectGitHub: vi.fn(), saveComputerConfiguration: vi.fn(), retryComputerSetup: vi.fn(), finishSetup: vi.fn(), submitStep: vi.fn(), ...extra }
}

function onboarding(store: SettingsStore, handlers: OnboardingActions, source: Partial<OnboardingSource> = {}, completed = false) {
  return <SettingsProvider store={store}><ApplicationCatalogProvider initialCatalog={fixtureApplicationCatalog}><SystemIntegrationProvider store={createFixtureSystemIntegrationStore(store)}><OnboardingApp
    source={{ ...onboardingScenarios.complete, ...source }}
    actions={handlers}
    githubConnectionState="disconnected"
    completed={completed}
  /></SystemIntegrationProvider></ApplicationCatalogProvider></SettingsProvider>
}

const footer = () => screen.getByRole("contentinfo", { name: "Onboarding actions" })
// The settings store applies what it was given on the next turn.
const settled = () => act(async () => { await Promise.resolve() })

describe("the agent step", () => {
  it("comes after the computers and says what each choice means in one sentence", async () => {
    const user = userEvent.setup()
    const handlers = actions()
    render(onboarding(createMemorySettingsStore({}, onComputers), handlers))
    await user.click(screen.getByRole("button", { name: "Continue" }))
    expect(handlers.submitStep).toHaveBeenCalledWith("computers", expect.anything())
    expect(screen.getByRole("tab", { name: /Your agent/ })).toHaveAttribute("aria-selected", "true")
    const group = screen.getByRole("radiogroup", { name: "Your agent" })
    expect(within(group).getAllByRole("radio").map(radio => radio.getAttribute("value"))).toEqual(["claude-code", "codex", "none"])
    expect(within(group).getByRole("radio", { name: "Claude Code" })).toBeVisible()
    expect(within(group).getByRole("radio", { name: "Codex" })).toBeVisible()
    expect(within(group).getByRole("radio", { name: "I'll set it up myself" })).toBeVisible()
    expect(group).toHaveTextContent("Anthropic's coding agent")
    expect(group).toHaveTextContent("OpenAI's coding agent")
    expect(group).toHaveTextContent("Silo installs nothing")
    expect(screen.getByRole("heading", { name: "Your agent", level: 2 })).toHaveAttribute("data-visual-heading", "hidden")
  })

  it("waits for a choice before Continue", async () => {
    const user = userEvent.setup()
    render(onboarding(createMemorySettingsStore({}, { ...onComputers, currentStep: "agent" }), actions()))
    await settled()
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled()
    expect(footer()).toHaveTextContent("Choose your agent")
    await user.click(screen.getByRole("radio", { name: "Codex" }))
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled()
    expect(footer()).toHaveTextContent("Continue to install Codex")
  })

  it.each([["Claude Code", "claude-code"], ["Codex", "codex"], ["I'll set it up myself", "none"]] as const)("remembers %s as the agent you use and submits it with the step", async (label, value) => {
    const user = userEvent.setup()
    const store = createMemorySettingsStore({}, { ...onComputers, currentStep: "agent" })
    const handlers = actions()
    render(onboarding(store, handlers))
    await user.click(screen.getByRole("radio", { name: label }))
    expect(store.getSnapshot().settings.agent).toBe(value)
    await user.click(screen.getByRole("button", { name: "Continue" }))
    expect(handlers.submitStep).toHaveBeenCalledExactlyOnceWith("agent", expect.objectContaining({ agent: value }))
    expect(screen.getByRole("tab", { name: /GitHub/ })).toHaveAttribute("aria-selected", "true")
  })

  it("starts from the agent remembered earlier", async () => {
    const store = createMemorySettingsStore({ agent: "codex" }, { ...onComputers, currentStep: "agent" })
    render(onboarding(store, actions()))
    await settled()
    expect(screen.getByRole("radio", { name: "Codex" })).toBeChecked()
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled()
  })

  it("sends the chosen agent along when setup finishes", async () => {
    const user = userEvent.setup()
    const handlers = actions()
    const store = createMemorySettingsStore({ agent: "claude-code" }, { ...onComputers, currentStep: "review" })
    render(onboarding(store, handlers, { readyToFinish: true }))
    await user.click(screen.getByRole("button", { name: "Finish" }))
    expect(handlers.finishSetup).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ agent: "claude-code" }))
  })

  it.each([
    ["queued", "Waiting to install Codex in main."],
    ["running", "Installing Codex in main: Downloading Codex (42%)"],
    ["succeeded", "Codex is ready in main."],
  ] as const)("shows the install while it is %s", async (status, text) => {
    const store = createMemorySettingsStore({ agent: "codex" }, { ...onComputers, currentStep: "agent" })
    render(onboarding(store, actions(), { agentSetup: { computer: "main", agent: "codex", status, step: "downloading", detail: "Downloading Codex (42%)" } }))
    await settled()
    expect(screen.getByRole("status", { name: "Agent install" })).toHaveTextContent(text)
    if (status === "running") expect(footer()).toHaveTextContent("In progress · Installing Codex in main")
    if (status === "succeeded") expect(footer()).toHaveTextContent("Complete · Codex is ready")
  })

  it("shows a failed install with Retry, and the step stays finishable", async () => {
    const user = userEvent.setup()
    const retryAgentInstall = vi.fn()
    const store = createMemorySettingsStore({ agent: "codex" }, { ...onComputers, currentStep: "agent" })
    render(onboarding(store, actions({ retryAgentInstall }), { agentSetup: { computer: "main", agent: "codex", status: "failed", failure: "The download could not be verified." } }))
    const alert = screen.getByRole("alert", { name: "Agent install" })
    expect(alert).toHaveTextContent("Codex could not be installed in main: The download could not be verified.")
    expect(alert).toHaveTextContent("You can still finish setup")
    await user.click(within(alert).getByRole("button", { name: "Retry" }))
    expect(retryAgentInstall).toHaveBeenCalledOnce()
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled()
  })
})

describe("the agent install in the setup model", () => {
  it("never blocks Finish: a failed agent leaves the queue and Finish as they were", () => {
    const base = { ...onboardingScenarios.complete, readyToFinish: true, setupQueue: (["computerRun", "computerVerify", "identityRun", "identityVerify", "githubRun", "githubVerify", "completion"] as const).map(id => ({ id, status: "succeeded" as const })) }
    const failed = { ...base, agentSetup: { computer: "main", agent: "codex" as const, status: "failed" as const, failure: "No network." } }
    const view = projectOnboarding(failed, "disconnected")
    expect(view.finishEnabled).toBe(true)
    expect(view.queueItems.every(({ status }) => status === "succeeded")).toBe(true)
    expect(view.stepStatus.agent).toBe("failed")
    expect(view.agent).toMatchObject({ status: "failed", failure: "No network." })
    expect(projectOnboarding(base, "disconnected").stepStatus.agent).toBe("waiting")
  })
})

describe("the final screen", () => {
  const done = { agentSetup: { computer: "main", agent: "claude-code" as const, status: "succeeded" as const } }

  it("makes Open the installed agent the main action, with Open Silo beside it", async () => {
    const user = userEvent.setup()
    const openAgent = vi.fn()
    render(onboarding(createMemorySettingsStore({ agent: "claude-code" }, { ...onComputers, currentStep: "review" }), actions({ openAgent }), done, true))
    const actionsBar = within(footer())
    const buttons = actionsBar.getAllByRole("button").map(button => button.textContent)
    expect(buttons).toEqual(["Open Claude Code in main", "Open Silo"])
    await user.click(actionsBar.getByRole("button", { name: "Open Claude Code in main" }))
    expect(openAgent).toHaveBeenCalledExactlyOnceWith("main", "claude-code")
    expect(screen.getByRole("group", { name: "Your agent" })).toHaveTextContent("Claude Code is ready in main.")
  })

  it("keeps Open Silo as the only action when no agent was installed", async () => {
    render(onboarding(createMemorySettingsStore({ agent: "none" }, { ...onComputers, currentStep: "review" }), actions({ openAgent: vi.fn() }), {}, true))
    await settled()
    expect(within(footer()).getAllByRole("button").map(button => button.textContent)).toEqual(["Open Silo"])
    expect(screen.getByRole("group", { name: "Your agent" })).toHaveTextContent("You set up the agent yourself.")
  })

  it("says the agent could not be installed and offers Retry, without taking Open Silo away", async () => {
    const user = userEvent.setup()
    const retryAgentInstall = vi.fn()
    render(onboarding(createMemorySettingsStore({ agent: "codex" }, { ...onComputers, currentStep: "review" }), actions({ retryAgentInstall, openAgent: vi.fn() }), { agentSetup: { computer: "main", agent: "codex", status: "failed", failure: "No network." } }, true))
    const alert = screen.getByRole("alert", { name: "Your agent" })
    expect(alert).toHaveTextContent("Codex could not be installed in main: No network.")
    await user.click(within(alert).getByRole("button", { name: "Retry" }))
    expect(retryAgentInstall).toHaveBeenCalledOnce()
    expect(within(footer()).getAllByRole("button").map(button => button.textContent)).toEqual(["Open Silo"])
  })

  it("shows the SSH name of the computer on request, for desktop apps that connect over SSH", async () => {
    const user = userEvent.setup()
    const ensureSshName = vi.fn(async () => ({ alias: "silo-6f0c1a52", includeInstalled: true, includeLine: "" }))
    render(onboarding(createMemorySettingsStore({ agent: "claude-code" }, { ...onComputers, currentStep: "review" }), actions({ ensureSshName, openAgent: vi.fn() }), done, true))
    expect(ensureSshName).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Show SSH name" }))
    expect(ensureSshName).toHaveBeenCalledExactlyOnceWith("main")
    expect(await screen.findByText("silo-6f0c1a52")).toBeVisible()
    expect(screen.getByRole("button", { name: "Copy SSH name" })).toBeVisible()
  })

  it("offers no SSH name while the agent is not installed", async () => {
    render(onboarding(createMemorySettingsStore({ agent: "codex" }, { ...onComputers, currentStep: "review" }), actions({ ensureSshName: vi.fn() }), { agentSetup: { computer: "main", agent: "codex", status: "failed", failure: "No network." } }, true))
    await settled()
    expect(screen.queryByRole("button", { name: "Show SSH name" })).not.toBeInTheDocument()
  })
})
