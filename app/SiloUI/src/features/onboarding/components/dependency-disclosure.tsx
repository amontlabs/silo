import { AlertCircle, Check, LoaderCircle, RefreshCw } from "lucide-react"
import { useState } from "react"

import { DisclosureHeader } from "@/components/disclosure-header"
import { ListCard, ListRowDetails, ListRowIcon } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleContent } from "@/components/ui/collapsible"
import { SetupNotice } from "@/features/onboarding/components/setup-notice"
import type { DependencyGroupView } from "@/features/onboarding/model/onboarding-state"

export function DependencyDisclosure({ group, onRetry }: { group: DependencyGroupView; onRetry?: () => void }) {
  const [disclosure, setDisclosure] = useState({ open: group.status === "failed", status: group.status })
  if (disclosure.status !== group.status) {
    setDisclosure({ open: group.status === "failed" || disclosure.open, status: group.status })
  }
  const pendingItems = group.items.filter(({ check }) => check?.status === "pending")
  const failedItems = group.items.filter(({ check }) => check && check.status !== "pass" && check.status !== "pending")
  const timedOut = failedItems.some(({ check }) => check?.status === "timeout")
  const unavailable = failedItems.some(({ check }) => check?.status === "unavailable")
  const caption = pendingItems.length > 0
    ? `Checking ${pendingItems.length} ${pendingItems.length === 1 ? "requirement" : "requirements"}…`
    : failedItems.length > 0
      ? timedOut && failedItems.length === 1 ? "Check timed out"
        : unavailable && failedItems.length === 1 ? "Checks unavailable"
          : `${failedItems.length} ${failedItems.length === 1 ? "check failed" : "checks failed"}`
      : `${group.items.length} of ${group.items.length} checks passed`

  return (
    <Collapsible open={disclosure.open} onOpenChange={(open) => setDisclosure({ open, status: group.status })} asChild>
      <ListCard>
        <DisclosureHeader
          icon={
            <ListRowIcon className={group.status === "failed" ? "bg-destructive/10 text-destructive" : group.status === "running" ? "" : "bg-success/10 text-success"}>
              {group.status === "running"
                ? <LoaderCircle className="size-3.5 animate-spin motion-reduce:animate-none" aria-label="Checking" />
                : group.status === "failed"
                ? <AlertCircle className="size-3.5" aria-label="Check failed" />
                : <Check className="size-3.5" aria-label="All checks passed" />}
            </ListRowIcon>
          }
          title={group.title}
          label={group.title}
          detail={caption}
        />
        <CollapsibleContent className="collapsible-content-motion">
          <ListRowDetails label={`${group.title} checks`} className="gap-3">
            <div className="grid gap-x-5 gap-y-3 sm:grid-cols-2">
              {group.items.map((item) => {
                const pending = item.check?.status === "pending"
                const failed = item.check && item.check.status !== "pass" && !pending
                const itemCaption = pending
                  ? "Checking…"
                  : item.check?.status === "timeout"
                    ? "Check timed out"
                    : item.check?.status === "unavailable"
                      ? "Check unavailable"
                      : item.check?.status === "failed" || item.check?.status === "needsAction"
                        ? "Check failed"
                        : item.check?.detail
                return (
                  <div key={item.name} className="grid grid-cols-[1rem_1fr] gap-1.5">
                    {pending
                      ? <LoaderCircle className="mt-0.5 size-3.5 animate-spin motion-reduce:animate-none text-muted-foreground" aria-label="Checking" />
                      : failed
                      ? <AlertCircle className="mt-0.5 size-3.5 text-destructive" aria-label="Failed" />
                      : <Check className="mt-0.5 size-3.5 text-muted-foreground" aria-label="Checked" />}
                    <div className="min-w-0">
                      <div className="text-[13px] leading-4 font-medium text-foreground">{item.name}</div>
                      <div className="truncate whitespace-nowrap text-[11px] leading-4 text-muted-foreground">{itemCaption}</div>
                    </div>
                  </div>
                )
              })}
            </div>
            {failedItems.map(({ name, check }) => check && (
              <SetupNotice
                key={name}
                title={check.title}
                detail={check.detail}
                recovery={check.remediation ?? undefined}
              />
            ))}
            {failedItems.length > 0 && onRetry && <div className="flex justify-end"><Button type="button" variant="outline" size="xs" onClick={onRetry}><RefreshCw aria-hidden="true" />Retry checks</Button></div>}
          </ListRowDetails>
        </CollapsibleContent>
      </ListCard>
    </Collapsible>
  )
}
