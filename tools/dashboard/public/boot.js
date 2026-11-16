// Runs before the body is parsed: marks that scripts run and picks the view
// from the address, so only that view is ever painted. Without scripts every
// view shows, in order.
(() => {
  const PREFIXES = [
    ['journey-', 'journeys'],
    ['product-', 'product'],
    ['item-', 'for-you'],
    ['change-', 'for-you'],
    ['note-', 'private'],
  ];
  const VIEWS = ['for-you', 'product', 'journeys', 'decisions', 'private'];
  function viewFor(hash) {
    const id = decodeURIComponent(String(hash || '').replace(/^#/, ''));
    if (VIEWS.includes(id)) return id;
    for (const [prefix, view] of PREFIXES) if (id.startsWith(prefix)) return view;
    return 'for-you';
  }
  window.siloViewFor = viewFor;
  document.documentElement.classList.add('js');
  document.documentElement.dataset.view = viewFor(location.hash);
})();
