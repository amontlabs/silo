import { ComputerUseSettings } from "@/features/application/components/computer-use-settings"
import { UpdatesCard } from "@/features/updates/updates"
import { StorageSection } from "@/features/storage/storage-section"
import { useLayoutEffect } from "react"
import { Accessibility, Paintbrush, Power } from "lucide-react"

import { ListCard, ListRow, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { PageContainer, PageHeader, SectionHeading } from "@/components/page"
import { FilterCombobox } from "@/components/filter-combobox"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import type { ApplicationSource } from "@/features/application/model/application-source"
import { defaultStartupComputerIds, startupComputerCandidates } from "@/features/application/model/startup-computers"
import { ApplicationPreferenceFields } from "@/features/preferences/components/application-preference-fields"
import { SettingsSaveNotice } from "@/features/preferences/components/settings-save-notice"
import type { ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"
import { useTheme } from "@/features/preferences/theme"
import { useSettings } from "@/features/preferences/settings-store"
import { useSystemIntegrations } from "@/features/preferences/system-integrations-store"

function SettingRow({ icon: Icon, title, description, control }: { icon: typeof Power; title: string; description: string; control: React.ReactNode }) {
  return (
    <ListRow
      className="row-hover"
      icon={<ListRowIcon aria-hidden="true"><Icon className="size-3.5" /></ListRowIcon>}
      title={<h4>{title}</h4>}
      detail={description}
      detailClassName="whitespace-normal"
      actions={control}
    />
  )
}

type GeneralPageProps = {
  source: ApplicationSource
  applicationPreferences: ApplicationPreferenceSelection
  onApplicationPreferencesChange: (preferences: ApplicationPreferenceSelection) => void
  reduceMotion: boolean
  onReduceMotionChange: (enabled: boolean) => void
  /** Whether the page is showing; computer use status is only watched then. */
  active?: boolean
}

/** Uses the application's settings store; the startup default only ever names local computers. */
export function GeneralPage({
  source,
  applicationPreferences,
  onApplicationPreferencesChange,
  reduceMotion,
  onReduceMotionChange,
  active = true,
}: GeneralPageProps) {
  const { theme, setTheme } = useTheme()
  const { settings, store, updateSettings } = useSettings()
  const integrations = useSystemIntegrations()
  const { startComputersAtLaunch: startAtLaunch } = settings
  const startupComputers = new Set(settings.startupComputerIds)

  useLayoutEffect(() => {
    store.updateDefaults({
      startupComputerIds: source.preferences.startupComputerIds ?? defaultStartupComputerIds(source.computers),
    })
  }, [store, source.computers, source.preferences.startupComputerIds])

  return (
    <PageContainer className="grid gap-4">
      <PageHeader title="General" />
      <SettingsSaveNotice />
      <UpdatesCard />
      <section className="grid gap-2">
        <SectionHeading>Appearance</SectionHeading>
        <ListCard>
          <SettingRow icon={Paintbrush} title="Theme" description="Choose an appearance or follow your system." control={
            <div className="w-40 max-w-[45%] shrink-0">
              <Select value={theme} onValueChange={setTheme}>
                <SelectTrigger size="sm" aria-label="Theme"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="system">System</SelectItem>
                  <SelectItem value="dark">Dark</SelectItem>
                  <SelectItem value="light">Light</SelectItem>
                </SelectContent>
              </Select>
            </div>
          } />
        </ListCard>
      </section>
      <section className="grid gap-2">
        <SectionHeading>Startup</SectionHeading>
        <ListCard divided>
          <div>
            <SettingRow icon={Power} title="Launch Silo at login" description="Keep computer status and notifications available." control={<Switch checked={integrations.loginEnabled} disabled={!integrations.initialized || integrations.loginPending || integrations.loginItem.state === "error" || integrations.loginItem.state === "unavailable"} onCheckedChange={(enabled) => { void integrations.setLaunchAtLogin(enabled) }} aria-label="Launch Silo at login" />} />
            {integrations.loginItem.state === "requiresApproval" && <ListRow
              icon={null}
              title="Approval required"
              detail="Allow Silo in Login Items to finish enabling this setting."
              detailClassName="whitespace-normal"
              actions={<Button type="button" variant="outline" size="xs" onClick={() => { void integrations.openIntegrationSettings("loginItem") }}>Open System Settings</Button>}
            />}
          </div>
          <div>
            <SettingRow icon={Power} title="Start computers at launch" description="Start selected computers when Silo opens." control={<Switch checked={startAtLaunch} onCheckedChange={(enabled) => { void updateSettings({ startComputersAtLaunch: enabled, ...(enabled ? { startupComputerIds: settings.startupComputerIds } : {}) }) }} aria-label="Start computers at launch" />} />
            {startAtLaunch && (
              <ListRowDetails label="Computers to start at launch" className="gap-2">
                <FilterCombobox
                  options={startupComputerCandidates(source.computers).map(({ configuration }) => ({ value: configuration.id, label: configuration.name }))}
                  selectedValues={startupComputers}
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
          </div>
        </ListCard>
      </section>
      <section className="grid gap-2">
        <SectionHeading>Applications</SectionHeading>
        <ListCard divided>
          <ApplicationPreferenceFields compact value={applicationPreferences} onChange={onApplicationPreferencesChange} />
        </ListCard>
      </section>
      <section className="grid gap-2">
        <SectionHeading>Accessibility</SectionHeading>
        <ListCard><SettingRow icon={Accessibility} title="Reduce motion" description="Disable nonessential interface animation." control={<Switch checked={reduceMotion} onCheckedChange={onReduceMotionChange} aria-label="Reduce motion" />} /></ListCard>
      </section>
      <div className="grid gap-4 empty:hidden"><ComputerUseSettings source={source} active={active} /></div>
      <StorageSection />
    </PageContainer>
  )
}
