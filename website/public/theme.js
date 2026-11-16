// Shared by the home page and the Workshop, before first paint.
(() => {
  const key = 'silo-website-theme';
  const system = matchMedia('(prefers-color-scheme: dark)');
  const valid = value => ['light', 'dark'].includes(value) ? value : 'system';
  let preference = 'system';
  try { preference = valid(localStorage.getItem(key)); } catch { /* Storage is optional. */ }
  function apply() {
    const dark = preference === 'dark' || (preference === 'system' && system.matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.documentElement.classList.toggle('dark', dark);
    document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', dark ? '#171a1b' : '#f7f7f4');
    document.querySelectorAll('[data-theme-trigger]').forEach(trigger => { trigger.title = `Color theme: ${preference[0].toUpperCase()}${preference.slice(1)}`; });
    document.querySelectorAll('[data-theme-option]').forEach(option => { option.setAttribute('aria-checked', String(option.dataset.themeOption === preference)); });
  }
  system.addEventListener('change', apply);
  addEventListener('storage', event => {
    if (event.key === key || event.key === null) { preference = valid(event.newValue); apply(); }
  });
  document.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('[data-theme-picker]').forEach(picker => {
      const trigger = picker.querySelector('[data-theme-trigger]');
      const menu = picker.querySelector('[role="menu"]');
      const options = [...picker.querySelectorAll('[data-theme-option]')];
      function close(restoreFocus = false) {
        menu.hidden = true;
        trigger.setAttribute('aria-expanded', 'false');
        if (restoreFocus) trigger.focus();
      }
      function open() {
        menu.hidden = false;
        trigger.setAttribute('aria-expanded', 'true');
        options.find(option => option.dataset.themeOption === preference).focus();
      }
      trigger.addEventListener('click', () => menu.hidden ? open() : close());
      trigger.addEventListener('keydown', event => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); open(); }
      });
      menu.addEventListener('keydown', event => {
        const index = options.indexOf(document.activeElement);
        const next = { ArrowDown: (index + 1) % options.length, ArrowUp: (index + options.length - 1) % options.length, Home: 0, End: options.length - 1 }[event.key];
        if (next !== undefined) { event.preventDefault(); options[next].focus(); }
        if (event.key === 'Escape') { event.preventDefault(); close(true); }
        if (event.key === 'Tab') close(true);
      });
      document.addEventListener('pointerdown', event => { if (!picker.contains(event.target)) close(); });
      picker.addEventListener('focusout', event => { if (!picker.contains(event.relatedTarget)) close(); });
      options.forEach(option => option.addEventListener('click', () => {
        preference = valid(option.dataset.themeOption);
        try { localStorage.setItem(key, preference); } catch { /* Still works for this visit. */ }
        apply();
        close(true);
      }));
    });
    apply();
  });
  apply();
})();
