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
    expect(screen.getByRole('heading', { level: 2, name: 'Other devices' })).toBeInTheDocument();
    expect(screen.getByText('Create and delete a computer')).toBeInTheDocument();
    expect(screen.getByText('A Linux server')).toBeInTheDocument();
  });
});
