import { afterEach, describe, expect, test, mock } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Play } from 'lucide-react';
import { Toolbar, ToolbarButton, ToolbarDivider, ToolbarLabel } from './Toolbar';

describe('<Toolbar />', () => {
  afterEach(cleanup);

  test('renders children with role=toolbar', () => {
    render(
      <Toolbar testid="t-1">
        <ToolbarButton icon={Play} label="Run" testid="btn-run" />
      </Toolbar>,
    );
    expect(screen.getByTestId('t-1').getAttribute('role')).toBe('toolbar');
    expect(screen.getByTestId('btn-run')).toBeTruthy();
  });

  test('button calls onClick', async () => {
    const fn = mock(() => {});
    render(<ToolbarButton icon={Play} label="Run" testid="btn-run" onClick={fn} />);
    await userEvent.click(screen.getByTestId('btn-run'));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  test('disabled button does not fire onClick', async () => {
    const fn = mock(() => {});
    render(<ToolbarButton icon={Play} label="Run" testid="btn-run" disabled onClick={fn} />);
    await userEvent.click(screen.getByTestId('btn-run'));
    expect(fn).not.toHaveBeenCalled();
  });

  test('active button reflects aria-pressed', () => {
    render(<ToolbarButton icon={Play} label="Run" testid="btn-run" active />);
    expect(screen.getByTestId('btn-run').getAttribute('aria-pressed')).toBe('true');
  });

  test('renders divider and label without crashing', () => {
    render(
      <Toolbar testid="t-2">
        <ToolbarLabel>group</ToolbarLabel>
        <ToolbarDivider />
      </Toolbar>,
    );
    expect(screen.getByTestId('t-2').textContent).toContain('group');
  });
});
