import { Component, type ReactNode } from 'react';
import { ErrorCard } from './ErrorCard';

interface State {
  error: Error | null;
}
interface Props {
  children: ReactNode;
  label?: string;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  reset = () => this.setState({ error: null });
  render() {
    if (this.state.error) {
      return (
        <ErrorCard
          message={`${this.props.label ?? 'Component'} crashed: ${this.state.error.message}`}
          cause={this.state.error}
          onRetry={this.reset}
        />
      );
    }
    return this.props.children;
  }
}
