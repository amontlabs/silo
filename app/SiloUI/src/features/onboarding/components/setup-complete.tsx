import { useLayoutEffect } from "react"
import { Bell, Boxes, Check, CircleAlert, CircleCheck, GitFork, HeartPulse, Power } from "lucide-react"

import { FilterCombobox } from "@/components/filter-combobox"
import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import { useSettings } from "@/features/preferences/settings-store"
import { useSystemIntegrations } from "@/features/preferences/system-integrations-store"
import type { SetupComputerConfiguration } from "@/contracts/silo"

const notificationCategories = [
  { id: "notifyFailures", label: "Failures", icon: CircleAlert },
  { id: "notifyChanges", label: "Unexpected computer changes", icon: HeartPulse },
  { id: "notifyCompletions", label: "Long tasks finished", icon: CircleCheck },
] as const

export function SetupComplete({ configurations, githubSummary }: {
  configurations: readonly SetupComputerConfiguration[]
  githubSummary: string
}) {
  const { settings, store, updateSettings } = useSettings()
  const integrations = useSystemIntegrations()
  const notificationsEnabled = settings.notificationsEnabled && integrations.notificationsAuthorized

  useLayoutEffect(() => {
    const initial = configurations.find(({ name }) => name === "dev") ?? configurations[0]
    store.updateDefaults({ startupComputerIds: initial ? [initial.id] : [] })
  }, [configurations, store])

  return (
    <section aria-labelledby="setup-complete-title" className="grid gap-3">
      <ListCard divided>
        <ListRow
          role="status"
          icon={<ListRowIcon className="bg-success/10 text-success" aria-hidden="true"><Check className="size-3.5" /></ListRowIcon>}
          title={<h2 id="setup-complete-title">Setup complete</h2>}
          detail={`${configurations.length} ${configurations.length === 1 ? "computer is" : "computers are"} ready. Open Silo to get started.`}
          detailClassName="whitespace-normal"
        />
        <ListRow
          icon={<ListRowIcon aria-hidden="true"><GitFork className="size-3.5" /></ListRowIcon>}
          title="GitHub access"
          detail={githubSummary}
          detailClassName="whitespace-normal"
        />
      </ListCard>
      <section aria-labelledby="setup-preferences-title" className="grid gap-2">
        <h3 id="setup-preferences-title" className="text-xs font-medium">Stay informed</h3>
        <ListCard divided>
          <div>
            <ListRow
              className="row-hover"
              icon={<ListRowIcon aria-hidden="true"><Power className="size-3.5" /></ListRowIcon>}
              title="Launch Silo at login"
              detail={null}
              actions={<Switch checked={integrations.loginEnabled} disabled={!integrations.initialized || integrations.loginPending || integrations.loginItem.state === "error" || integrations.loginItem.state === "unavailable"} onCheckedChange={(enabled) => { void integrations.setLaunchAtLogin(enabled) }} aria-label="Launch Silo at login" />}
            />
            {integrations.loginItem.state === "requiresApproval" && <ListRow
              className="py-1.5"
              icon={null}
              title={<span className="text-xs">Approval required</span>}
              detail={null}
              actions={<Button type="button" variant="outline" size="xs" onClick={() => { void integrations.openIntegrationSettings("loginItem") }}>Open System Settings</Button>}
            />}
            {integrations.loginEnabled && (
              <ListRowDetails label="Startup preferences" className="mx-0 gap-0 px-0 py-1">
                <ListRow
                  className="py-1.5 row-hover"
                  icon={<ListRowIcon aria-hidden="true"><Boxes className="size-3.5" /></ListRowIcon>}
                  title={<span className="text-xs">Start computers at launch</span>}
                  detail={null}
                  actions={<Switch checked={settings.startComputersAtLaunch} onCheckedChange={(enabled) => { void updateSettings({ startComputersAtLaunch: enabled, ...(enabled ? { startupComputerIds: settings.startupComputerIds } : {}) }) }} aria-label="Start computers at launch" />}
                />
                {settings.startComputersAtLaunch && (
                  <ListRowDetails label="Computers to start at launch" className="mx-0 gap-2 px-2">
                    <FilterCombobox
                      options={configurations.map((configuration) => ({ value: configuration.id, label: configuration.name }))}
                      selectedValues={new Set(settings.startupComputerIds)}
                      onChange={(selected) => { void updateSettings({ startupComputerIds: [...selected] }) }}
                      label="Startup computers"
                      inputLabel="Add computer at startup"
                      placeholder="Select computers…"
                      listLabel="Available startup computers"
                      selectedLabel="Selected startup computers"
                      emptyMessage="No computers available."
                    />
                  </ListRowDetails>
                )}
              </ListRowDetails>
            )}
          </div>
          <div>
            <ListRow
              className="row-hover"
              icon={<ListRowIcon aria-hidden="true"><Bell className="size-3.5" /></ListRowIcon>}
              title="Enable notifications"
              detail={null}
              actions={<Switch checked={notificationsEnabled} disabled={!integrations.initialized || integrations.notificationsPending || integrations.notifications.state === "error" || integrations.notifications.state === "unavailable"} onCheckedChange={(enabled) => { void integrations.setNotificationsEnabled(enabled) }} aria-label="Enable notifications" />}
            />
            {integrations.notifications.state === "denied" && <ListRow
              className="py-1.5"
              icon={null}
              title={<span className="text-xs">Blocked in System Settings</span>}
              detail={null}
              actions={<Button type="button" variant="outline" size="xs" onClick={() => { void integrations.openIntegrationSettings("notifications") }}>Open System Settings</Button>}
            />}
            {notificationsEnabled && (
              <ListRowDetails label="Alert categories" className="mx-0 gap-0 px-0 py-1">
                {notificationCategories.map(({ id, label, icon: Icon }) => (
                  <ListRow
                    key={id}
                    className="py-1.5 row-hover"
                    icon={<ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon>}
                    title={<span className="text-xs">{label}</span>}
                    detail={null}
                    actions={<Switch checked={settings[id]} onCheckedChange={(enabled) => { void updateSettings({ [id]: enabled }) }} aria-label={label} />}
                  />
                ))}
              </ListRowDetails>
            )}
          </div>
        </ListCard>
      </section>
    </section>
  )
}
