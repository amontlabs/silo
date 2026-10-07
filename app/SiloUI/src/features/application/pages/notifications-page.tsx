import { Bell, CircleAlert, CircleCheck, HeartPulse } from "lucide-react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import { useSettings } from "@/features/preferences/settings-store"
import { useSystemIntegrations } from "@/features/preferences/system-integrations-store"

const categories = [
  { id: "notifyFailures", label: "Failures", detail: "Actions and background work that fail, such as start, push, computer export, or computer import.", icon: CircleAlert },
  { id: "notifyChanges", label: "Unexpected computer changes", detail: "A computer stops, fails, or recovers without you asking.", icon: HeartPulse },
  { id: "notifyCompletions", label: "Long tasks finished", detail: "Work that took more than a few seconds finishes while Silo is in the background.", icon: CircleCheck },
] as const

export function NotificationsPage() {
  const { settings, updateSettings } = useSettings()
  const integrations = useSystemIntegrations()
  const enabled = settings.notificationsEnabled && integrations.notificationsAuthorized

  return (
    <div className="mx-auto grid w-full max-w-4xl gap-4 px-4 py-5 sm:px-6 sm:py-6">
      <h2 className="text-xs font-medium">Notifications</h2>
      <ListCard>
        <ListRow
          className="row-hover"
          icon={<ListRowIcon aria-hidden="true"><Bell className="size-3.5" /></ListRowIcon>}
          title={<h3>Enable notifications</h3>}
          detail="Silo sends system notifications while its window is in the background. While you're using Silo, results appear in the app."
          detailClassName="whitespace-normal"
          actions={<Switch checked={enabled} disabled={!integrations.initialized || integrations.notificationsPending || integrations.notifications.state === "error" || integrations.notifications.state === "unavailable"} onCheckedChange={(checked) => { void integrations.setNotificationsEnabled(checked) }} aria-label="Enable notifications" />}
        />
      </ListCard>
      {integrations.notifications.state === "denied" && <ListCard><ListRow
        icon={null}
        title="Blocked in System Settings"
        detail="Allow notifications for Silo before enabling alerts."
        detailClassName="whitespace-normal"
        actions={<Button type="button" variant="outline" size="xs" onClick={() => { void integrations.openIntegrationSettings("notifications") }}>Open System Settings</Button>}
      /></ListCard>}
      <section className="grid gap-2">
        <h3 className="text-xs font-medium">Alert categories</h3>
        <ListCard divided>
          {categories.map(({ id, label, detail, icon: Icon }) => (
            <ListRow
              key={id}
              className="row-hover"
              icon={<ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon>}
              title={<h4>{label}</h4>}
              detail={detail}
              detailClassName="whitespace-normal"
              actions={<Switch checked={settings[id]} onCheckedChange={(checked) => { void updateSettings({ [id]: checked }) }} disabled={!enabled} aria-label={label} />}
            />
          ))}
        </ListCard>
      </section>
    </div>
  )
}
