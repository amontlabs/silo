import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import type { ApplicationGitHubConfiguration } from "@/features/application/model/application-source"
import { createMemorySettingsStore, SettingsProvider } from "@/features/preferences/settings-store"
import { SystemIntegrationProvider } from "@/features/preferences/system-integrations-store"
import { createFixtureSystemIntegrationStore } from "@/fixtures/system-integrations"
import { onboardingScenarios } from "@/fixtures/scenarios"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { assertNativeBridgeMocksHandled, nativeBridgeMock } from "@/test/native-bridge-mock"
import { ProductionOnboarding } from "./production-onboarding"
import { createProductionSource, useProductionSource, type ProductionBridge, type ProductionSource } from "./production-source"

afterEach(assertNativeBridgeMocksHandled)

function Setup({ source }: { source: ProductionSource }) {
  const snapshot = useProductionSource(source)
  return <ProductionOnboarding source={source} application={snapshot.source} dependencies={{ checks: onboardingScenarios.complete.preflightChecks, retry: () => {} }} />
}

it.each(["connected", "disconnected"] as const)("keeps token policy through the real onboarding save bridge and completion with OAuth %s", async (oauthState) => {
  const user = userEvent.setup()
  const live = applicationSourceForScenario("running", oauthState)
  live.computers = [live.computers[0]]
  const policy = { ...live.github.computers![0], authenticationMethod: "token" as const, tokenSecretId: "token-1", repositoryMode: "selected" as const, allRepositoriesAllowChanges: false }
  live.secrets = [{ id: "token-1", name: "work", kind: "github-token", computers: [], allowedDomains: [], state: "active", account: "alice", connection: { state: "connected" } }]
  live.github = { ...live.github, computers: [policy], policyRevision: 10 }
  const invoke = nativeBridgeMock({
    read_application_state: () => structuredClone(live),
    read_backup_state: () => ({ snapshotId: "test", availability: "available", archives: [], operation: null }),
    read_setup_activity: () => [],
    read_network_state: () => ({ computers: [] }),
    remote_network_state: () => ({ computers: [] }),
    device_list: () => [],
    connections_status: () => ({ enabled: false, deviceId: "local", name: "Laptop", address: "developer@laptop" }),
    read_operation_queue: () => ({ running: [], waiting: [] }),
    verify_computer_identities: () => true,
    configure_computer_identities: () => undefined,
    save_github_configuration: (args) => {
      const configuration = args!.configuration as ApplicationGitHubConfiguration
      live.github = { ...live.github, policyRevision: 11, computers: configuration.computers,
        computerOperations: [{ computer: "dev", status: "succeeded", message: "Applied" }] }
      return structuredClone(live.github)
    },
  })
  const source = createProductionSource({ invoke, listen: async () => () => {} } as ProductionBridge)
  await source.initialize()
  const settings = createMemorySettingsStore()
  const integrations = createFixtureSystemIntegrationStore(settings)
  const view = render(<SettingsProvider store={settings}><SystemIntegrationProvider store={integrations}><Setup source={source} /></SystemIntegrationProvider></SettingsProvider>)
  try {
    await user.click(screen.getByRole("tab", { name: /GitHub/ }))
    expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeChecked()
    expect(screen.getByRole("radio", { name: "Use token for dev" })).toBeEnabled()
    await user.click(screen.getByRole("button", { name: "Continue" }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_github_configuration", {
      configuration: { baseRevision: 10, deviceIdentity: live.github.deviceIdentity ?? null, computers: [policy] },
    }))
    expect(source.getSnapshot().source?.github.computers).toEqual([policy])
    if (oauthState === "disconnected") {
      expect(screen.getByRole("group", { name: "GitHub access" })).toHaveTextContent("Personal token in 1 computer")
      expect(screen.getByRole("group", { name: "GitHub access" })).not.toHaveTextContent("Skipped")
    }
    await waitFor(() => expect(screen.getByRole("button", { name: "Finish" })).toBeEnabled())
    await user.click(screen.getByRole("button", { name: "Finish" }))
    await waitFor(() => expect(settings.getSnapshot().settings.onboardingComplete).toBe(true))
    expect(source.getSnapshot().source?.github.computers).toEqual([policy])
  } finally {
    view.unmount()
    source.dispose()
  }
})
