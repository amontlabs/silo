import { SectionHeading } from "@/components/page"
import { Switch } from "@/components/ui/switch"
import { useComputerUseBridge } from "@/desktop/computer-use-bridge"
import { useSettings } from "@/features/preferences/settings-store"

function NewComputerApprovalSetting() {
  const { settings, updateSettings } = useSettings()
  if (!useComputerUseBridge()) return null
  return <section aria-label="Computer use" className="grid gap-3">
    <SectionHeading>Computer use</SectionHeading>
    <div className="rounded-lg border p-3">
      <div className="flex items-center justify-between gap-4"><div><label htmlFor="computer-use-auto-approval" className="text-xs font-medium">Allow agents to use the desktop without asking in new computers</label><p className="text-xs text-muted-foreground">Claude Code, Codex and similar agents stop asking before using the computer’s desktop. Not a security boundary.</p></div><Switch id="computer-use-auto-approval" checked={settings.computerUseAutoApproval} onCheckedChange={enabled => { void updateSettings({ computerUseAutoApproval: enabled }) }} /></div>
    </div>
  </section>
}

/** The computer use settings: the approval default for new computers. */
export function ComputerUseSettings() {
  return <NewComputerApprovalSetting />
}
