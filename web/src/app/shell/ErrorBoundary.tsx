import { Component, type ReactNode } from 'react';
import { StatePage } from '../../components/StatePage/StatePage';

export interface ErrorBoundaryProps {
  resetKey: string;
  children: ReactNode;
}

interface ErrorBoundaryState {
  failed: boolean;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidUpdate(previous: ErrorBoundaryProps) {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  reset = () => {
    this.setState({ failed: false });
  };

  render() {
    if (this.state.failed) {
      return <StatePage kind="error" onRetry={this.reset} />;
    }
    return this.props.children;
  }
}
