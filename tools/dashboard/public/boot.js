// Runs before the body is parsed: marks that scripts run and picks the view
// from the address, so only that view is ever painted. Without scripts every
// view shows, in order. Any address, even a malformed one, gives a view.
(() => {
  const PREFIXES = [
    ['journey-', 'journeys'],
    ['product-', 'product'],
    ['orphan-journeys-', 'journeys'],
    ['orphan-product-', 'product'],
    ['item-', 'for-you'],
    ['change-', 'for-you'],
    ['note-', 'private'],
  ];
  const VIEWS = ['for-you', 'product', 'journeys', 'decisions', 'private'];
  function viewFor(hash) {
    let id;
    try {
      id = decodeURIComponent(String(hash || '').replace(/^#/, ''));
    } catch {
      return 'for-you';
    }
    if (VIEWS.includes(id)) return id;
    for (const [prefix, view] of PREFIXES) if (id.startsWith(prefix)) return view;
    return 'for-you';
  }
  window.siloViewFor = viewFor;
  let view = 'for-you';
  try {
    view = viewFor(location.hash);
  } catch {
    view = 'for-you';
  }
  document.documentElement.dataset.view = view;
  document.documentElement.classList.add('js');
})();
