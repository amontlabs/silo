// Which row's relations are drawn: the row under the pointer (after a rest of 120 ms), else the one holding the keyboard's
// focus, else the row opened last. A tiny store, so the strip and the rail redraw without rendering any row.

/** How long the pointer rests on a row before its relations are drawn. */
export const POINT_MS = 120;

export interface RelationStore {
  /** The instance key of the row whose relations are drawn, or null. */
  active: () => string | null;
  point: (key: string | null) => void;
  focus: (key: string | null) => void;
  open: (key: string | null) => void;
  subscribe: (listener: () => void) => () => void;
}

export function relationStore(): RelationStore {
  let pointer: string | null = null;
  let focused: string | null = null;
  let opened: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const listeners = new Set<() => void>();
  let last: string | null = null;
  const emit = () => {
    const now = pointer ?? focused ?? opened;
    if (now === last) return;
    last = now;
    for (const listener of listeners) listener();
  };
  return {
    active: () => pointer ?? focused ?? opened,
    point: (key) => {
      clearTimeout(timer);
      if (key === null) {
        pointer = null;
        emit();
        return;
      }
      timer = setTimeout(() => {
        pointer = key;
        emit();
      }, POINT_MS);
    },
    focus: (key) => {
      focused = key;
      emit();
    },
    open: (key) => {
      opened = key;
      emit();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
