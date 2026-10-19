import { errorMessage } from "@/lib/error-message"

export interface DirectoryEntry {
  name: string
  path: string
  kind: 'folder' | 'file' | 'symlink'
}

export interface DirectoryPage {
  snapshotId: string
  entries: DirectoryEntry[]
  nextOffset: number | null
}

export type DirectoryLoader = (computer: string, path: string, offset: number, snapshotId?: string) => Promise<DirectoryPage>
export interface DirectorySnapshot {
  snapshotId: string | null
  loadingMore: boolean
  errorOperation: 'load' | 'refresh' | 'more' | null
  entries: DirectoryEntry[] | null
  nextOffset: number | null
  loading: boolean
  error: string | null
}

const safeErrors = new Set([
  'Could not load this folder.', 'Folder listing expired. Refresh this folder.',
  'This folder no longer exists.', 'Permission denied.', 'This folder cannot be browsed.',
  'This folder is too large to list.', 'Start this computer to browse its files.', 'Invalid folder request.',
  'Folder changed. Reload to continue.',
])
const emptySnapshot: DirectorySnapshot = { snapshotId: null, loadingMore: false, errorOperation: null, entries: null, nextOffset: null, loading: false, error: null }
export const directoryKey = (computer: string, path: string) => JSON.stringify([computer, path])

type RecordState = {
  computer: string
  snapshot: DirectorySnapshot
  listeners: Set<() => void>
  generation: number
  pending: Promise<void> | null
}

export function createDirectoryStore(loader?: DirectoryLoader) {
  const records = new Map<string, RecordState>()
  const queue: { record: RecordState; run: () => Promise<void>; finish: () => void }[] = []
  let active = 0
  let disposed = false

  function prune() {
    let inactive = [...records.values()].filter(record => !record.listeners.size && !record.pending).length
    for (const [key, record] of records) {
      if (inactive <= 128) break
      if (!record.listeners.size && !record.pending) {
        records.delete(key)
        inactive--
      }
    }
  }

  function getRecord(key: string) {
    let record = records.get(key)
    if (!record) {
      const [computer] = JSON.parse(key) as [string, string]
      record = { computer, snapshot: emptySnapshot, listeners: new Set(), generation: 0, pending: null }
    }
    records.delete(key)
    records.set(key, record)
    return record
  }

  function update(record: RecordState, snapshot: DirectorySnapshot) {
    record.snapshot = snapshot
    for (const listener of record.listeners) listener()
  }

  function drain() {
    while (!disposed && active < 3 && queue.length) {
      const job = queue.shift()!
      active++
      void job.run().finally(() => {
        active--
        job.finish()
        prune()
        drain()
      })
    }
  }

  function cancelQueued(record: RecordState) {
    for (let index = queue.length - 1; index >= 0; index--) {
      if (queue[index].record === record) queue.splice(index, 1)[0].finish()
    }
  }

  function invalidate(record: RecordState) {
    record.generation++
    record.pending = null
    cancelQueued(record)
    update(record, emptySnapshot)
  }

  function load(computer: string, path: string, options: { more?: boolean; refresh?: boolean } = {}): Promise<void> {
    if (disposed) return Promise.resolve()
    const record = getRecord(directoryKey(computer, path))
    if (record.pending) return record.pending
    const previous = record.snapshot
    if (previous.entries && !options.refresh && (!options.more || previous.nextOffset === null)) return Promise.resolve()
    const generation = record.generation
    const current = () => !disposed && record.generation === generation
    let finish!: () => void
    const pending = new Promise<void>(resolve => { finish = resolve })
    record.pending = pending
    const operation = options.refresh ? 'refresh' : options.more && previous.entries !== null ? 'more' : 'load'
    update(record, { ...previous, loading: true, loadingMore: operation === 'more' })
    queue.push({ record, finish, run: async () => {
      try {
        if (!current()) return
        if (!loader) throw new Error('unavailable')
        const more = options.more && !options.refresh && previous.entries !== null
        let offset = more ? previous.nextOffset! : 0
        let snapshotId = more ? previous.snapshotId : null
        let entries: DirectoryEntry[] = []
        let nextOffset: number | null = null
        // Refresh every already loaded page, publishing only once all succeed.
        do {
          const page = await loader(computer, path, offset, snapshotId ?? undefined)
          if (!current()) return
          if (!page.snapshotId || (snapshotId !== null && page.snapshotId !== snapshotId)) throw new Error('Folder changed. Reload to continue.')
          snapshotId = page.snapshotId
          if (page.nextOffset !== null && page.nextOffset <= offset) throw new Error('invalid pagination')
          entries.push(...page.entries)
          nextOffset = page.nextOffset
          if (more || !options.refresh || entries.length >= (previous.entries?.length ?? 0) || nextOffset === null) break
          offset = nextOffset
        } while (nextOffset !== null)
        if (more) entries = [...previous.entries!, ...entries]
        entries = [...new Map(entries.map(entry => [entry.path, entry])).values()]
        update(record, { entries, nextOffset, snapshotId, loading: false, loadingMore: false, error: null, errorOperation: null })
      } catch (error) {
        const message = errorMessage(error)
        const retryOperation = message === 'Folder listing expired. Refresh this folder.' || message === 'Folder changed. Reload to continue.' ? 'refresh' : operation
        if (current()) update(record, { ...previous, loading: false, loadingMore: false, errorOperation: retryOperation, error: safeErrors.has(message) ? message : loader ? 'Could not load this folder.' : 'Files are unavailable.' })
      } finally {
        if (current()) record.pending = null
      }
    } })
    drain()
    return pending
  }

  return {
    setLoader(next: DirectoryLoader | undefined) { loader = next },
    subscribe(key: string, listener: () => void) {
      if (disposed) return () => {}
      const record = getRecord(key)
      record.listeners.add(listener)
      prune()
      return () => { record.listeners.delete(listener); prune() }
    },
    getSnapshot(key: string): DirectorySnapshot { return records.get(key)?.snapshot ?? emptySnapshot },
    load,
    invalidateComputer(computer: string) {
      for (const record of records.values()) if (record.computer === computer) invalidate(record)
      prune()
    },
    dispose() {
      disposed = true
      for (const record of records.values()) invalidate(record)
      records.clear()
    },
  }
}
