declare module 'virtual:workshop-data' {
  const data: import('./plan').BuildData;
  export default data;
}

declare module 'virtual:workshop-notes' {
  const notes: import('./plan').Notes;
  export default notes;
}

declare module 'virtual:workshop-facts' {
  const facts: import('./plan').Facts;
  export default facts;
}

declare module 'virtual:workshop-history' {
  const histories: import('./plan').Histories;
  export default histories;
}

/** The id of this build, also in the page's `meta` (chunks.ts); empty on the dev server and in tests. */
declare const __WORKSHOP_BUILD__: string;
