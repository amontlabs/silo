import { SectionHeading } from "@/components/page"
import type { CredentialUsage } from "@/desktop/production-schemas"

export const credentialsNote = "Forks get none of the credentials Silo manages unless you name them; files on disk are copied."

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`

/** Which credentials the fork used and which attempts were refused, or "none". */
export function CredentialsUsed({ credentials }: { credentials: CredentialUsage }) {
  const { used, blocked } = credentials
  return (
    <section className="grid gap-1.5" aria-label="Credentials used">
      <SectionHeading>Credentials used</SectionHeading>
      {used.length === 0 && blocked.length === 0 ? <p className="text-sm">none</p> : (
        <ul className="grid gap-1 text-sm">
          {used.map(use => (
            <li key={`${use.secret}:${use.host}:${use.method}`}>
              {use.secret} → {use.host} ({use.method}, {plural(use.count, "time")})
            </li>
          ))}
          {blocked.map(block => (
            <li key={`blocked:${block.host}:${block.method}:${block.reason}`} className="text-destructive">
              Blocked: {block.host} ({block.method}, {plural(block.count, "attempt")}) — {block.reason}
            </li>
          ))}
        </ul>
      )}
      <p className="text-caption text-muted-foreground">{credentialsNote}</p>
    </section>
  )
}

/** The count of changes in each bucket, e.g. "code 2". */
export function BucketCounts({ counts }: { counts: Record<string, number> }) {
  const entries = Object.entries(counts)
  return (
    <section className="grid gap-1.5" aria-label="Changes by bucket">
      <SectionHeading>Changes</SectionHeading>
      {entries.length === 0 ? <p className="text-sm">No changes</p> : (
        <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
          {entries.map(([bucket, count]) => <li key={bucket}><span className="text-muted-foreground">{bucket}</span> {count}</li>)}
        </ul>
      )}
    </section>
  )
}
