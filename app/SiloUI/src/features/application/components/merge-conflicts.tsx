import { useState } from "react"

import { SectionHeading } from "@/components/page"
import { Button } from "@/components/ui/button"
import type { MergeChoice } from "@/desktop/production-source"
import type { KeepConflict } from "@/desktop/production-schemas"
import { choiceLabel } from "@/features/application/model/merge-copy"

const isChoice = (value: string): value is MergeChoice => value === "fork" || value === "main" || value === "branch"
const answerable = (conflict: KeepConflict) => Boolean(conflict.id) && conflict.allowed.some(isChoice)

/**
 * The items both sides changed. Each one the engine accepts an answer for gets one choice;
 * items no answer settles (credentials, a database) show what the engine reports. Merging again
 * needs an answer for every item that takes one.
 */
export function MergeConflicts({ conflicts, forkName, busy, onMerge }: {
  conflicts: readonly KeepConflict[]
  forkName: string
  busy: boolean
  onMerge: (choices: Record<string, MergeChoice>) => void
}) {
  const [choices, setChoices] = useState<Record<string, MergeChoice>>({})
  const questions = conflicts.filter(answerable)
  const unanswered = questions.filter(({ id }) => !choices[id]).length
  return (
    <section className="grid gap-3" aria-label="Conflicts">
      <SectionHeading>Conflicts</SectionHeading>
      <ul className="grid gap-3">
        {conflicts.map((conflict, index) => (
          <li key={conflict.id || `${conflict.kind}:${conflict.item}:${index}`} className="grid gap-1.5 rounded-md border p-3 text-sm">
            {answerable(conflict) ? (
              <fieldset className="grid gap-1.5">
                <legend className="font-medium break-all">{conflict.item || conflict.id}</legend>
                <ConflictFacts conflict={conflict} />
                {conflict.allowed.filter(isChoice).map(choice => (
                  <label key={choice} className="flex items-start gap-2">
                    <input
                      type="radio"
                      className="mt-0.5"
                      name={`conflict:${conflict.id}`}
                      checked={choices[conflict.id] === choice}
                      disabled={busy}
                      onChange={() => setChoices(current => ({ ...current, [conflict.id]: choice }))}
                    />
                    <span>{choiceLabel(choice, forkName)}</span>
                  </label>
                ))}
              </fieldset>
            ) : (
              <>
                <p className="font-medium break-all">{conflict.item || conflict.kind}</p>
                <ConflictFacts conflict={conflict} />
                <p className="text-caption text-muted-foreground">No answer settles this one; Silo does not bring it over.</p>
              </>
            )}
          </li>
        ))}
      </ul>
      {questions.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={busy || unanswered > 0} onClick={() => onMerge(choices)}>Merge with these choices</Button>
          {unanswered > 0 && <span className="text-caption text-muted-foreground">Choose for {unanswered} more {unanswered === 1 ? "item" : "items"}.</span>}
        </div>
      )}
    </section>
  )
}

function ConflictFacts({ conflict }: { conflict: KeepConflict }) {
  return (
    <>
      {conflict.detail && <p className="text-muted-foreground">{conflict.detail}</p>}
      {(conflict.forkChange || conflict.mainChange) && (
        <p className="text-caption text-muted-foreground">
          {conflict.forkChange && <>Fork: {conflict.forkChange}. </>}
          {conflict.mainChange && <>Main: {conflict.mainChange}.</>}
        </p>
      )}
    </>
  )
}
