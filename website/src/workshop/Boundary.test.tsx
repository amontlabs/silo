import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Boundary } from './Boundary';
import { buildData } from './fixtures';

function Broken(): never {
  throw new Error('render failed');
}

describe('the error boundary', () => {
  it('leaves the plan readable from the built-in data when the page fails to render', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    render(<Boundary data={buildData()}><Broken /></Boundary>);
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('Part C is being built.')).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Part E' })).toBeInTheDocument();
    const part = (name: string) => screen.getByRole('region', { name });
    expect(part('Part B')).toHaveTextContent('FeaturesCreate and delete a computer (feature, not decided)');
    expect(part('Part A')).toHaveTextContent('FeaturesThe engine runs on its own (feature, decided)Every call defined once (feature, decided)DecisionsWhich language is the core written in? (decision, decided)');
    expect(part('Part C')).toHaveTextContent('FeaturesYour shell inside (feature, being built)');
    expect(part('Part E')).toHaveTextContent('Which format does an export use? (decision, dropped)');
    expect(part('Product')).toHaveTextContent('How is security built? (decision, decided)Which host runs Silo first? (decision, not decided)Which logs does Silo keep? (decision, replaced) Replaced by How are logs kept honest?');
  });
});
