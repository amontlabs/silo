import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import './workshop.css';

const loadNotes = () => import('virtual:workshop-notes').then((module) => module.default);

function mount() {
  const root = document.getElementById('workshop-root');
  if (!root) return;
  createRoot(root).render(
    <StrictMode>
      <Boundary data={data}>
        <App data={data} loadNotes={loadNotes} accountSlot={document.getElementById('workshop-account')} />
      </Boundary>
    </StrictMode>,
  );
}

const params = new URLSearchParams(location.search);
if (import.meta.env.DEV && params.has('fixture')) {
  void import('./devFixture').then(({ installFixture }) => {
    installFixture(data, params);
    mount();
  });
} else mount();
