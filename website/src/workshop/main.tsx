import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import featuresText from '../../../docs/product/features.jsonl?raw';
import stagesText from '../../../docs/product/stages.json?raw';
import decisionsText from '../../../docs/product/DECISIONS.md?raw';
import { App } from './App';
import { parseDecisions, parseFeatures, parseStages } from './plan';
import './workshop.css';

const data = {
  features: parseFeatures(featuresText),
  stages: parseStages(stagesText),
  decided: parseDecisions(decisionsText),
};

const root = document.getElementById('workshop-root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App data={data} accountSlot={document.getElementById('workshop-account')} />
    </StrictMode>,
  );
}
