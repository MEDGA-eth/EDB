import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import { SessionEndedOverlay } from './SessionEndedOverlay';
import { useSession } from '../store/session';

describe('<SessionEndedOverlay />', () => {
  beforeEach(() => useSession.getState().setSessionEnded(false));
  afterEach(cleanup);

  test('renders nothing when sessionEnded is false', () => {
    render(<SessionEndedOverlay />);
    expect(screen.queryByTestId('session-ended')).toBeNull();
  });

  test('renders modal when sessionEnded is true', () => {
    useSession.getState().setSessionEnded(true);
    render(<SessionEndedOverlay />);
    const dialog = screen.getByTestId('session-ended');
    expect(dialog).toBeTruthy();
    expect(dialog.getAttribute('role')).toBe('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.textContent).toContain('Session ended');
  });
});
