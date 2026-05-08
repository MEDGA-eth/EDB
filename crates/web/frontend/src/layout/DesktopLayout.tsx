import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import { CodePanel } from '../components/panels/CodePanel';
import { TracePanel } from '../components/panels/TracePanel';
import { DisplayPanel } from '../components/panels/DisplayPanel';
import { TerminalPanel } from '../components/panels/TerminalPanel';

export function DesktopLayout() {
  return (
    <div className="h-full" data-testid="desktop-layout">
      <PanelGroup direction="vertical" className="h-full">
        <Panel defaultSize={50} minSize={20}>
          <PanelGroup direction="horizontal">
            <Panel defaultSize={60} minSize={20}>
              <CodePanel />
            </Panel>
            <PanelResizeHandle className="w-1 bg-(--color-border)" />
            <Panel defaultSize={40} minSize={15}>
              <TracePanel />
            </Panel>
          </PanelGroup>
        </Panel>
        <PanelResizeHandle className="h-1 bg-(--color-border)" />
        <Panel defaultSize={50} minSize={20}>
          <PanelGroup direction="horizontal">
            <Panel defaultSize={50} minSize={20}>
              <DisplayPanel />
            </Panel>
            <PanelResizeHandle className="w-1 bg-(--color-border)" />
            <Panel defaultSize={50} minSize={20}>
              <TerminalPanel />
            </Panel>
          </PanelGroup>
        </Panel>
      </PanelGroup>
    </div>
  );
}
