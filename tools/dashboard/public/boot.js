// Runs before the body is parsed: marks that scripts run and picks the view
// from the address, so only that view is ever painted. Without scripts every
// view shows, in order. Any address, even a malformed one, gives a view: an
// empty or unknown one gives the page's default view (the Stack when the
// feature list exists, else For you), which the server writes on <html>.
(() => {
  const PREFIXES = [
    ['product-', 'product'],
    ['orphan-product-', 'product'],
    ['change-', 'product'],
    ['mark-', 'product'],
    ['item-', 'for-you'],
    ['earlier-', 'for-you'],
    ['note-', 'private'],
    ['feature-', 'stack'],
    ['stack-', 'stack'],
    ['where-', 'stack'],
    ['verdicts-', 'decisions'],
  ];
  const VIEWS = ['stack', 'for-you', 'decisions', 'product', 'private'];
  const root = document.documentElement;
  const fallback = root.dataset.default === 'stack' ? 'stack' : 'for-you';
  function viewFor(hash) {
    let id;
    try {
      id = decodeURIComponent(String(hash || '').replace(/^#/, ''));
    } catch {
      return fallback;
    }
    if (VIEWS.includes(id)) return id;
    for (const [prefix, view] of PREFIXES) if (id.startsWith(prefix)) return view;
    return fallback;
  }
  window.siloViewFor = viewFor;
  window.siloDefaultView = fallback;
  let view = fallback;
  try {
    view = viewFor(location.hash);
  } catch {
    view = fallback;
  }
  root.dataset.view = view;
  root.classList.add('js');
})();
