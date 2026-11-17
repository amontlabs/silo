import { StrictMode } from 'react';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import { takeOver } from './takeover';
import './workshop.css';

const loadNotes = () => import('virtual:workshop-notes').then((module) => module.default);

function mount() {
  const root = document.getElementById('workshop-root');
  if (!root) return;
  const slot = document.getElementById('workshop-account');
  takeOver(root, slot, (
    <StrictMode>
      <Boundary data={data}>
        <App data={data} loadNotes={loadNotes} accountSlot={slot} />
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
