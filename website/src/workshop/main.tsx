import { StrictMode } from 'react';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import { takeOver } from './takeover';
import { chunk } from './chunks';
import './workshop.css';

// The plan as the page holds it, for scripts/load-check.mjs, which reads from it what the page must show (its parts,
// the whole product's rows). It is the public plan, already in this script.
(window as Window & { __workshopData?: unknown }).__workshopData = data;

// This script's path, which the page of another deployment does not name.
const script = new URL(import.meta.url).pathname;
const loadNotes = chunk(() => import('virtual:workshop-notes').then((module) => module.default), script);
const loadFacts = chunk(() => import('virtual:workshop-facts').then((module) => module.default), script);
const loadHistory = chunk(() => import('virtual:workshop-history').then((module) => module.default), script);

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
