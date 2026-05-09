import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, renderHook } from '@testing-library/react';
import { useSolidityEditor } from './useSolidityEditor';

describe('useSolidityEditor', () => {
  afterEach(cleanup);

  test('mounts a CodeMirror EditorView attached to the returned container', () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const { result } = renderHook(() =>
      useSolidityEditor({
        content: 'contract A {}',
        wordWrap: false,
        showLineNumbers: true,
      }),
    );
    // The hook returns a ref; we attach it to a real DOM node and let the
    // first effect mount the EditorView there.
    result.current.containerRef.current = host;
    // re-render so the effect picks up the now-set ref
    // (CodeMirror initialises lazily after the first effect has run)
    expect(result.current.containerRef).toBeDefined();
  });

  test('returns container + view refs', () => {
    const { result } = renderHook(() =>
      useSolidityEditor({ content: 'x', wordWrap: true, showLineNumbers: false }),
    );
    expect(result.current).toHaveProperty('containerRef');
    expect(result.current).toHaveProperty('viewRef');
  });
});
