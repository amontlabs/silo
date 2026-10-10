import { Button } from "@/components/ui/button"
import { SectionHeading } from "@/components/page"
import { ListCard } from "@/components/list-row"
import { DependencyDisclosure } from "@/features/onboarding/components/dependency-disclosure"
import type { DependencyGroupView } from "@/features/onboarding/model/onboarding-state"
import { ApplicationPreferenceFields } from "@/features/preferences/components/application-preference-fields"
import type { ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"

export function DependenciesStep({
  groups,
  applicationPreferences,
  onApplicationPreferencesChange,
  onRetry,
  onConnectDevice,
}: {
  groups: DependencyGroupView[]
  applicationPreferences: ApplicationPreferenceSelection
  onApplicationPreferencesChange: (preferences: ApplicationPreferenceSelection) => void
  onRetry?: () => void
  onConnectDevice?: () => void
}) {
  const retryGroup = groups.find(({ status }) => status === "failed")?.id
  return (
    <section aria-labelledby="dependencies-title">
      <h2 id="dependencies-title" className="sr-only" data-visual-heading="hidden">Dependencies</h2>
      {onConnectDevice && <div className="mb-5 grid justify-items-start gap-2">
        <Button variant="outline" size="sm" onClick={onConnectDevice}>Connect another device…</Button>
        <p className="text-xs text-muted-foreground">Use computers on another device without setting up local computers.</p>
      </div>}
      <div className="grid gap-2">
        {groups.map((group) => <DependencyDisclosure key={group.id} group={group} onRetry={group.id === retryGroup ? onRetry : undefined} />)}
      </div>
      <section aria-labelledby="onboarding-applications-title" className="mt-5 grid gap-2">
        <SectionHeading id="onboarding-applications-title">Applications</SectionHeading>
        <ListCard divided>
          <ApplicationPreferenceFields compact value={applicationPreferences} onChange={onApplicationPreferencesChange} />
        </ListCard>
      </section>
    </section>
  )
}
