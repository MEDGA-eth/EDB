import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ActivityBar } from './ActivityBar';
import { useSession } from '../store/session';

describe('<ActivityBar />', () => {
  afterEach(() => {
    cleanup();
    useSession.setState({ activeActivity: 'explorer' });
  });

  test('renders all five activity buttons', () => {
    render(<ActivityBar />);
    expect(screen.getByTestId('activity-explorer')).toBeTruthy();
    expect(screen.getByTestId('activity-trace')).toBeTruthy();
    expect(screen.getByTestId('activity-variables')).toBeTruthy();
    expect(screen.getByTestId('activity-terminal')).toBeTruthy();
    expect(screen.getByTestId('activity-breakpoints')).toBeTruthy();
  });

  test('clicking sets activeActivity in the store', async () => {
    render(<ActivityBar />);
    await userEvent.click(screen.getByTestId('activity-trace'));
    expect(useSession.getState().activeActivity).toBe('trace');
  });

  test('active item has aria-pressed=true', () => {
    useSession.setState({ activeActivity: 'terminal' });
    render(<ActivityBar />);
    expect(screen.getByTestId('activity-terminal').getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByTestId('activity-explorer').getAttribute('aria-pressed')).toBe('false');
  });
});
