import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import { ConnectionIndicator } from './ConnectionIndicator';
import { useSession } from '../store/session';

describe('<ConnectionIndicator />', () => {
  beforeEach(() => useSession.getState().setConnection('connected'));
  afterEach(cleanup);

  test('renders connected state', () => {
    render(<ConnectionIndicator />);
    expect(screen.getByTestId('connection-indicator').getAttribute('data-state')).toBe('connected');
  });

  test('reflects degraded state', () => {
    useSession.getState().setConnection('degraded');
    render(<ConnectionIndicator />);
    expect(screen.getByTestId('connection-indicator').getAttribute('data-state')).toBe('degraded');
  });

  test('reflects offline state', () => {
    useSession.getState().setConnection('offline');
    render(<ConnectionIndicator />);
    expect(screen.getByTestId('connection-indicator').getAttribute('data-state')).toBe('offline');
  });
});
