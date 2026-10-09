// Tab isolation (§51, ADR-0016): a broken server page, plugin editor, or
// any view that throws must not take the shell down. The boundary wraps
// ONE mounted tab content — a crash renders the recoverable "this tab
// crashed" page; the strip, the other tabs, and zamind are untouched.
// Reload (§60) rebuilds the view and never touches a server process, so
// it is the recovery verb here too.
//
// §10 (P0): the details stay inspectable. The boundary is the safety
// net, not the burial — the error's message and the component stack are
// one disclosure away, because a crash that cannot be examined cannot
// be reported, and a crash that cannot be reported cannot be fixed.

import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTabs } from "../../state/tabs";
import { Button } from "../../ui/Button";
import styles from "./TabCrash.module.css";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
  componentStack: string | null;
}

function TabCrashed({
  onReload,
  error,
  componentStack,
}: {
  onReload: () => void;
  error: Error | null;
  componentStack: string | null;
}) {
  const message = error?.message || String(error);
  return (
    <div className={styles.page} role="alert">
      <h2 className={styles.title}>This tab crashed</h2>
      <p className={styles.copy}>
        The page hit an error and could not keep going. Other tabs are unaffected. Reloading
        rebuilds this view from ZIM's service — the server process itself is never touched.
      </p>
      <p className={styles.errorSentence}>
        <code>{message}</code>
      </p>
      <Button variant="primary" onClick={onReload}>
        Reload this tab
      </Button>
      {message !== "" ? (
        <details className={styles.details}>
          <summary>Technical details</summary>
          <pre className={styles.stack}>
            {message}
            {componentStack ? `\n\nComponent stack:${componentStack}` : ""}
          </pre>
        </details>
      ) : null}
    </div>
  );
}

export class TabBoundary extends Component<Props, State> {
  state: State = { error: null, componentStack: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // The crash stays inside this tab's mount; the log line is for the
    // devtools console only — never a rethrow, never a shell exit.
    console.error("zamin: a tab's view crashed", error, info.componentStack);
    this.setState({ componentStack: info.componentStack ?? null });
  }

  render() {
    if (this.state.error) {
      return (
        <TabCrashed
          onReload={() => {
            this.setState({ error: null, componentStack: null });
            useTabs.getState().reload();
          }}
          error={this.state.error}
          componentStack={this.state.componentStack}
        />
      );
    }
    return this.props.children;
  }
}
