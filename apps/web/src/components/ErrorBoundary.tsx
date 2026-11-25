import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
  componentStack: string;
}

/**
 * Last-resort UI boundary: a render crash should show a readable recovery card
 * instead of a blank page (and keep the error visible for bug reports).
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null, componentStack: '' };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error, componentStack: '' };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Surface in devtools and in the DOM card; never swallow entirely.
    console.error('[openmesh] UI crashed', error, info.componentStack);
    this.setState({ componentStack: info.componentStack ?? '' });
  }

  render() {
    const { error, componentStack } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="flex min-h-screen items-center justify-center bg-ink-950 p-6">
        <div className="w-full max-w-xl rounded-2xl border border-rose-400/25 bg-rose-400/[0.04] p-6">
          <div className="flex items-center gap-2">
            <span className="grid h-9 w-9 place-items-center rounded-xl bg-rose-400/15 text-rose-300">!</span>
            <div>
              <h1 className="text-base font-semibold text-rose-100">The OpenMesh UI hit a snag</h1>
              <p className="text-xs text-rose-200/70">
                Your messages are safe — the bridge owns the store. Reload to resync.
              </p>
            </div>
          </div>
          <pre className="mt-4 max-h-64 overflow-auto rounded-lg border border-rose-400/15 bg-ink-950/60 p-3 font-mono text-2xs leading-relaxed text-rose-200/70">
            {error.message}
            {'\n'}
            {error.stack?.split('\n').slice(1, 6).join('\n')}
            {'\n\nComponent stack:'}
            {componentStack.split('\n').slice(0, 14).join('\n')}
          </pre>
          <div className="mt-4 flex gap-2">
            <button
              type="button"
              onClick={() => window.location.reload()}
              className="rounded-lg bg-rose-400 px-3 py-1.5 text-xs font-semibold text-rose-950 hover:bg-rose-300"
            >
              Reload the app
            </button>
            <button
              type="button"
              onClick={() => this.setState({ error: null })}
              className="rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-white/[0.08]"
            >
              Try again
            </button>
          </div>
          <p className="mt-3 text-3xs text-rose-200/40">
            If this persists, note the message above and file an issue with the OpenMesh project.
          </p>
        </div>
      </div>
    );
  }
}
