// Tab isolation (§51, ADR-0016): a broken server page, plugin editor, or
// any view that throws must not take the shell down. The boundary wraps
// ONE mounted tab content — a crash renders the recoverable "this tab
// crashed" page; the strip, the other tabs, and zamind are untouched.
// Reload (§60) rebuilds the view and never touches a server process, so
// it is the recovery verb here too.

import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTabs } from "../../state/tabs";
import { Button } from "../../ui/Button";
import styles from "./TabCrash.module.css";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

function TabCrashed({ onReload }: { onReload: () => void }) {
  return (
    <div className={styles.page} role="alert">
      <h2 className={styles.title}>This tab crashed</h2>
      <p className={styles.copy}>
        The page hit an error and could not keep going. Other tabs are unaffected. Reloading
        rebuilds this view from the daemon — the server process itself is never touched.
      </p>
      <Button variant="primary" onClick={onReload}>
        Reload this tab
      </Button>
    </div>
  );
}

export class TabBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // The crash stays inside this tab's mount; the log line is for the
    // devtools console only — never a rethrow, never a shell exit.
    console.error("zamin: a tab's view crashed", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <TabCrashed
          onReload={() => {
            this.setState({ error: null });
            useTabs.getState().reload();
          }}
        />
      );
    }
    return this.props.children;
  }
}
