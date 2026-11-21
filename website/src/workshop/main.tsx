import { StrictMode } from 'react';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import { takeOver } from './takeover';
import { chunk } from './chunks';
import { takeReopen } from './memory';
import './workshop.css';

// The plan as the page holds it, for scripts/load-check.mjs, which reads from it what the page must show (its parts,
// the whole product's rows). It is the public plan, already in this script.
(window as Window & { __workshopData?: unknown }).__workshopData = data;

// A chunk that fails asks the site whether it now serves another build than this one (chunks.ts).
const build = __WORKSHOP_BUILD__;
const loadNotes = chunk(() => import('virtual:workshop-notes').then((module) => module.default), build);
const loadFacts = chunk(() => import('virtual:workshop-facts').then((module) => module.default), build);
const loadHistory = chunk(() => import('virtual:workshop-history').then((module) => module.default), build);

// The rows the page's own reload asked to open again, read once per load.
const reopen = takeReopen();

function mount() {
  const root = document.getElementById('workshop-root');
  if (!root) return;
  const slot = document.getElementById('workshop-account');
  takeOver(root, slot, (
    <StrictMode>
      <Boundary data={data}>
        <App data={data} loadNotes={loadNotes} loadFacts={loadFacts} loadHistory={loadHistory} accountSlot={slot} reopen={reopen} />
      </Boundary>
    </StrictMode>
  ));
}

const params = new URLSearchParams(location.search);
if (import.meta.env.DEV && params.has('fixture')) {
  void import('./devFixture').then(({ installFixture }) => {
    installFixture(data, params);
    mount();
  });
} else mount();
