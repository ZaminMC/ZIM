// Window identity (ADR-0018). ZIM is one SPA that can live in
// several browsing contexts at once — §50's "Move tab to new window"
// opens a second ZIM window, and two windows must never fight
// over one localStorage slot. Each context therefore owns its strip
// under its own storage key, keyed by an identity minted once per boot.
//
// The identity lives in sessionStorage: it survives a reload (the strip
// must survive it too) and dies with the tab. The hard part is telling
// a RELOAD from a BIRTH that was handed a copy of the opener's
// sessionStorage (browsers copy it when a context opens with an opener):
//   - a handoff birth (the §50 move, `#handoff=` in the URL) always
//     mints fresh — it must never share the origin's key;
//   - a plain birth mints fresh whenever the platform can say so
//     (Navigation Timing type "navigate");
//   - a reload (type "reload"/"back_forward") reuses the stored id;
//   - when the platform stays silent about navigation (old webviews,
//     test DOMs), an existing id is REUSED — losing a strip on reload
//     is the one unrecoverable mistake this module refuses to make.

const WINDOW_KEY = "zim.window";

let currentId: string | null = null;

function navigationType(): string | null {
  try {
    const entries = performance.getEntriesByType("navigation") as
      | Array<{ type?: string }>
      | undefined;
    const first = entries && entries.length > 0 ? entries[0] : undefined;
    return typeof first?.type === "string" ? first.type : null;
  } catch {
    return null;
  }
}

function mint(): string {
  return `w${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Birth vs reload. Called exactly once per boot, before the tabs store
 *  is created (its storage keys every strip off this identity). */
export function resolveWindowIdentity(hash: string | null | undefined): void {
  const handoffBirth = typeof hash === "string" && hash.includes("handoff=");
  try {
    const existing = sessionStorage.getItem(WINDOW_KEY);
    const nav = navigationType();
    const reload = nav === "reload" || nav === "back_forward";
    if (!handoffBirth && existing !== null && (reload || nav === null)) {
      currentId = existing;
      return;
    }
    const id = mint();
    sessionStorage.setItem(WINDOW_KEY, id);
    currentId = id;
  } catch {
    // A denied sessionStorage must not kill the boot: an in-memory id
    // keeps this context working; its strip just does not outlive it.
    if (currentId === null) currentId = mint();
  }
}

/** The identity the storage layer keys strips with. Boot must have run;
 *  if it somehow has not (tests, hot reloads), a lazy id is minted. */
export function currentWindowId(): string {
  if (currentId === null) resolveWindowIdentity(null);
  return currentId as string;
}

/** Test hook: force an identity (or clear it so the next boot re-mints). */
export function __setWindowIdentityForTests(id: string | null): void {
  currentId = id;
}
