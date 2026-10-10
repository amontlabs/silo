import { render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import { onboardingScenarios } from "@/fixtures/scenarios"
import { projectOnboarding } from "@/features/onboarding/model/onboarding-state"
import type { OnboardingSource } from "@/features/onboarding/model/onboarding-source"
import { OnboardingFooter } from "./onboarding-footer"

const setupQueue: NonNullable<OnboardingSource["setupQueue"]> = ["computerRun", "computerVerify", "identityRun", "identityVerify", "completion"].map((id) => ({ id: id as NonNullable<OnboardingSource["setupQueue"]>[number]["id"], status: "idle" }))

describe("onboarding queue feedback", () => {
  it("distinguishes idle setup from submitted work", () => {
    const viewModel = projectOnboarding({ ...onboardingScenarios.complete, setupQueue }, "disconnected")
    const view = render(<OnboardingFooter activeStep="computers" viewModel={viewModel} onBack={vi.fn()} onContinue={vi.fn()} />)
    expect(screen.getByText("Not started · Continue to create your computers. The first time can take a few minutes.")).toBeVisible()
    expect(screen.queryByText("Waiting · Setup tasks are queued")).not.toBeInTheDocument()
    view.rerender(<OnboardingFooter activeStep="dependencies" viewModel={viewModel} onBack={vi.fn()} onContinue={vi.fn()} />)
    expect(screen.getByText("Ready · Continue to configure computers")).toBeVisible()
    view.rerender(<OnboardingFooter activeStep="review" viewModel={{ ...viewModel, finishEnabled: true }} onBack={vi.fn()} onContinue={vi.fn()} />)
    expect(screen.getByText("Ready · Finish setup")).toBeVisible()
    const running = projectOnboarding({ ...onboardingScenarios.complete, setupQueue: setupQueue.map((item) => item.id === "computerRun" ? { ...item, status: "running" } : item) }, "disconnected")
    view.rerender(<OnboardingFooter activeStep="github" viewModel={running} onBack={vi.fn()} onContinue={vi.fn()} />)
    expect(screen.getByText("In progress · Create computers")).toBeVisible()
  })

  it("keeps completed computer work complete when returning from another step", () => {
    const viewModel = projectOnboarding({ ...onboardingScenarios.complete, setupQueue: setupQueue.map((item) => item.id === "computerRun" || item.id === "computerVerify" ? { ...item, status: "succeeded" } : item) }, "disconnected")
    const props = { viewModel, onBack: vi.fn(), onContinue: vi.fn() }
    const view = render(<OnboardingFooter {...props} activeStep="review" />)
    view.rerender(<OnboardingFooter {...props} activeStep="computers" />)
    expect(screen.getByText("Complete · Computers are ready")).toBeVisible()
    expect(screen.queryByText("Not started · Continue to create your computers. The first time can take a few minutes.")).not.toBeInTheDocument()
    view.rerender(<OnboardingFooter {...props} activeStep="github" />)
    expect(screen.getByText("Not started · Continue to save GitHub access and Git identities")).toBeVisible()
    view.rerender(<OnboardingFooter {...props} activeStep="computers" />)
    expect(screen.getByText("Complete · Computers are ready")).toBeVisible()
  })

  it("projects only explicit operations and does not invent GitHub completion", () => {
    const view = projectOnboarding({ ...onboardingScenarios.complete, setupQueue }, "disconnected")
    expect(view.queueItems.map(({ id }) => id)).toEqual(setupQueue.map(({ id }) => id))
    expect(view.queueItems.every(({ status }) => status === "idle")).toBe(true)
    expect(view.computerProgress.currentMessage).toBe("Continue to create computers")
    expect(view.computerProgress.computers.every(({ status }) => status !== "ready")).toBe(true)
    expect(view.computerProgress.totalOperations).toBe(onboardingScenarios.complete.computerConfigurations.length * 2)
  })
})
