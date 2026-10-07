// Boot order is load order (ADR-0018): the window identity must exist
// before the tabs store is created (its storage keys every strip off
// it), and the §50 handoff must be claimed before the first render so a
// moved tab arrives in its new window without a flash of the new-tab
// page. This module is main.tsx's FIRST import for exactly those
// reasons; nothing else may pull tabs.ts earlier.
import { claimHandoff } from "../state/tabs";
import { resolveWindowIdentity } from "../state/windowIdentity";

resolveWindowIdentity(window.location.hash);
if (claimHandoff(window.location.hash)) {
  // The handoff hash is consumed state, not an address: a reload of this
  // window must boot as a plain reload, not re-read a dead slot.
  history.replaceState(null, "", window.location.pathname + window.location.search);
}
