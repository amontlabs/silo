import { initializeTour } from "./tour.js";

const tabs = [...document.querySelectorAll('[role="tab"]')];
function selectWorkflow(tab, focus = false) {
  for (const item of tabs) {
    const selected = item === tab;
    item.setAttribute("aria-selected", String(selected));
    item.tabIndex = selected ? 0 : -1;
    document.getElementById(item.getAttribute("aria-controls")).hidden =
      !selected;
  }
  if (focus) tab.focus();
}
for (const [index, tab] of tabs.entries()) {
  tab.addEventListener("click", () => selectWorkflow(tab));
  tab.addEventListener("keydown", (event) => {
    const next = {
      ArrowDown: (index + 1) % tabs.length,
      ArrowUp: (index + tabs.length - 1) % tabs.length,
      Home: 0,
      End: tabs.length - 1,
    }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    selectWorkflow(tabs[next], true);
  });
}

initializeTour(document);
