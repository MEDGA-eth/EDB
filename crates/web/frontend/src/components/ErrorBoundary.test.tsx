import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ErrorBoundary } from './ErrorBoundary';

function Boom({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) throw new Error('kaboom');
  return <div data-testid="ok-child">ok</div>;
}

describe('<ErrorBoundary />', () => {
  // Silence React's error logging from the throwing child.
  let originalError: typeof console.error;
  beforeEach(() => {
    originalError = console.error;
    console.error = () => {};
  });
  afterEach(() => {
    console.error = originalError;
    cleanup();
  });

  test('renders children when no error', () => {
    render(
      <ErrorBoundary label="Test">
        <Boom shouldThrow={false} />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId('ok-child')).toBeTruthy();
  });

  test('renders ErrorCard when child throws', () => {
    render(
      <ErrorBoundary label="Test">
        <Boom shouldThrow={true} />
      </ErrorBoundary>,
    );
    const card = screen.getByTestId('error-card');
    expect(card).toBeTruthy();
    expect(card.textContent).toContain('Test crashed');
    expect(card.textContent).toContain('kaboom');
  });

  test('clicking Retry resets and remounts the child', async () => {
    let shouldThrow = true;
    function Child() {
      return <Boom shouldThrow={shouldThrow} />;
    }
    render(
      <ErrorBoundary label="Test">
        <Child />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId('error-card')).toBeTruthy();
    shouldThrow = false;
    await userEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(screen.getByTestId('ok-child')).toBeTruthy();
  });
});
