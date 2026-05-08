import { afterEach, describe, expect, test, mock } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ErrorCard } from './ErrorCard';

describe('<ErrorCard />', () => {
  afterEach(cleanup);

  test('renders message without code', () => {
    render(<ErrorCard message="something went wrong" />);
    const card = screen.getByTestId('error-card');
    expect(card).toBeTruthy();
    expect(card.textContent).toContain('Error');
    expect(card.textContent).not.toContain('(');
    expect(card.textContent).toContain('something went wrong');
  });

  test('renders with code in header when provided', () => {
    render(<ErrorCard code={-32000} message="boom" />);
    const card = screen.getByTestId('error-card');
    expect(card.textContent).toContain('Error (-32000)');
    expect(card.textContent).toContain('boom');
  });

  test('does not render Retry when onRetry is omitted', () => {
    render(<ErrorCard message="no retry here" />);
    expect(screen.queryByRole('button', { name: /retry/i })).toBeNull();
  });

  test('clicking Retry invokes onRetry', async () => {
    const onRetry = mock(() => {});
    render(<ErrorCard message="please retry" onRetry={onRetry} />);
    await userEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
