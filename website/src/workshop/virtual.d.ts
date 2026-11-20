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
