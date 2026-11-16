import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import data from 'virtual:workshop-data';
import { App } from './App';
import { Boundary } from './Boundary';
import './workshop.css';

const root = document.getElementById('workshop-root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Boundary data={data}>
        <App data={data} accountSlot={document.getElementById('workshop-account')} />
      </Boundary>
    </StrictMode>,
  );
}
