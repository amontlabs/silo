import { StrictMode } from 'react';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import { takeOver } from './takeover';
import './workshop.css';

// The plan as the page holds it, for scripts/load-check.mjs, which reads from it what the page must show (its parts,
// the whole product's rows). It is the public plan, already in this script.
(window as Window & { __workshopData?: unknown }).__workshopData = data;

const loadNotes = () => import('virtual:workshop-notes').then((module) => module.default);
const loadFacts = () => import('virtual:workshop-facts').then((module) => module.default);
const loadHistory = () => import('virtual:workshop-history').then((module) => module.default);

function mount() {
  const root = document.getElementById('workshop-root');
  if (!root) return;
  const slot = document.getElementById('workshop-account');
  takeOver(root, slot, (
    <StrictMode>
      <Boundary data={data}>
        <App data={data} loadNotes={loadNotes} loadFacts={loadFacts} loadHistory={loadHistory} accountSlot={slot} />
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
