import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HelpOverlay } from './HelpOverlay';

describe('<HelpOverlay />', () => {
  afterEach(cleanup);

  test('overlay is hidden initially', () => {
    render(<HelpOverlay />);
    expect(screen.queryByTestId('help-overlay')).toBeNull();
  });

  test('clicking the ? button opens the overlay', async () => {
    render(<HelpOverlay />);
    await userEvent.click(screen.getByTestId('help-open'));
    expect(screen.getByTestId('help-overlay')).toBeTruthy();
  });

  test('clicking Close button closes the overlay', async () => {
    render(<HelpOverlay />);
    await userEvent.click(screen.getByTestId('help-open'));
    expect(screen.getByTestId('help-overlay')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(screen.queryByTestId('help-overlay')).toBeNull();
  });

  test('clicking the backdrop closes the overlay', async () => {
    render(<HelpOverlay />);
    await userEvent.click(screen.getByTestId('help-open'));
    const backdrop = screen.getByTestId('help-overlay');
    expect(backdrop).toBeTruthy();
    await userEvent.click(backdrop);
    expect(screen.queryByTestId('help-overlay')).toBeNull();
  });

  test('clicking inside the panel does NOT close the overlay', async () => {
    render(<HelpOverlay />);
    await userEvent.click(screen.getByTestId('help-open'));
    const panel = screen.getByTestId('help-panel');
    await userEvent.click(panel);
    expect(screen.getByTestId('help-overlay')).toBeTruthy();
  });
});
