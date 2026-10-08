// The feedback lane (ADR-0028): an operator-typed report — title, details,
// an optional screenshot — sent to the development repo as a real issue.
//
// The rules the store keeps:
//   1. Two honest routes. With a GitHub token configured (Settings), the
//      report POSTs to the issues API from the panel itself; without one,
//      the composed report rides the browser instead — the GitHub issue
//      form the operator is already signed into. Neither route pretends
//      the other happened.
//   2. The token is a machine-local secret. It rides only to
//      api.github.com, never into an issue body, an error message, a log
//      line, or the browser URL. §47's credential discipline, applied to
//      our own lane.
//   3. GitHub's issues API cannot attach images. A screenshot therefore
//      travels the clipboard: the page copies it back and the operator
//      pastes it into GitHub's own form (browser route at send time; the
//      created issue page for the token route). The page says so, in
//      words, every time.
//   4. Every failure is a typed state with a next step (§81) — never a
//      silent success, never a dead end.

import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

export const ISSUES_API_URL = "https://api.github.com/repos/ZaminMC/ZaminPanel/issues";
export const USER_API_URL = "https://api.github.com/user";
export const NEW_ISSUE_URL = "https://github.com/ZaminMC/ZaminPanel/issues/new";

/** The static facts every report carries — the page gathers them from the
 *  same sources the About page reads, so the issue's evidence is the
 *  install's truth. */
export interface FeedbackIdentity {
  installedVersion: string | null;
  platform: string;
}

export type SignInState = "unknown" | "checking" | "signed-in" | "invalid";

export type SendOutcome =
  | { kind: "idle" }
  | { kind: "created"; url: string; issueNumber: number }
  | { kind: "browser"; url: string }
  | { kind: "error"; note: string };

interface FeedbackState {
  /** Machine-local GitHub token (Settings owns the field). Empty = the
   *  browser route. Never rendered anywhere but the settings mask. */
  token: string;
  login: string | null;
  signIn: SignInState;
  /** The POST in flight — the page reads it to disable Send exactly once. */
  sending: boolean;
  setToken: (token: string) => void;
  /** Validate the stored token against GET /user; typed outcomes only. */
  checkSignIn: () => Promise<SignInState>;
  /** The issue body: what the operator typed plus the diagnostics block
   *  (version, platform) — deterministic and tested. */
  composeBody: (details: string, identity: FeedbackIdentity, hasScreenshot: boolean) => string;
  /** The prefilled browser URL for the no-token route. */
  composeBrowserUrl: (title: string, body: string) => string;
  /** Send: the token route POSTs the issue; the browser route returns the
   *  prepared URL. The caller performs the OS calls (open, clipboard). */
  send: (input: {
    title: string;
    details: string;
    identity: FeedbackIdentity;
    hasScreenshot: boolean;
  }) => Promise<SendOutcome>;
}

/** The diagnostics block appended to every report (both routes). */
export function diagnosticsBlock(identity: FeedbackIdentity, hasScreenshot: boolean): string {
  const lines = [
    "",
    "---",
    "",
    `- ZaminPanel: ${identity.installedVersion ?? "(the host has not answered yet)"}`,
    `- Platform: ${identity.platform}`,
    `- Route: sent from the panel's feedback page`,
    hasScreenshot
      ? `- Screenshot: attached via GitHub's paste-attach (the API cannot carry images)`
      : `- Screenshot: none`,
  ];
  return lines.join("\n");
}

/** Guard the browser URL against GitHub's address-bar limits: truncate the
 *  body honestly and say so, rather than silently losing the tail. */
export function composeBrowserUrlChecked(
  base: string,
  title: string,
  body: string,
  limit = 4000,
): { url: string; truncated: boolean } {
  const params = new URLSearchParams();
  params.set("title", title);
  if (body.length <= limit) {
    params.set("body", body);
    return { url: `${base}?${params.toString()}`, truncated: false };
  }
  params.set("body", `${body.slice(0, limit)}\n\n…(the report was too long for the address bar — the tail was cut; paste the rest if it matters)`);
  return { url: `${base}?${params.toString()}`, truncated: true };
}

export const useFeedback = create<FeedbackState>()(
  persist(
    (set, get) => ({
      token: "",
      login: null,
      signIn: "unknown",
      sending: false,

      setToken: (token: string) => {
        // A new token is unproven until /user says otherwise.
        set({ token: token.trim(), login: null, signIn: "unknown" });
      },

      checkSignIn: async () => {
        const token = get().token;
        if (token === "") {
          set({ login: null, signIn: "unknown" });
          return "unknown";
        }
        set({ signIn: "checking" });
        try {
          const response = await fetch(USER_API_URL, {
            headers: {
              Authorization: `Bearer ${token}`,
              Accept: "application/vnd.github+json",
              "X-GitHub-Api-Version": "2022-11-28",
            },
          });
          if (response.status === 401) {
            set({ login: null, signIn: "invalid" });
            return "invalid";
          }
          if (!response.ok) {
            // Rate limit, outage — the token may still be fine; say so.
            set({ signIn: "unknown" });
            return "unknown";
          }
          const body = (await response.json()) as { login?: string };
          const login = typeof body.login === "string" ? body.login : null;
          set({ login, signIn: login ? "signed-in" : "unknown" });
          return login ? "signed-in" : "unknown";
        } catch {
          set({ signIn: "unknown" });
          return "unknown";
        }
      },

      composeBody: (details: string, identity: FeedbackIdentity, hasScreenshot: boolean): string => {
        const text = details.trim();
        return `${text}${diagnosticsBlock(identity, hasScreenshot)}\n`;
      },

      composeBrowserUrl: (title: string, body: string): string => {
        return composeBrowserUrlChecked(NEW_ISSUE_URL, title, body).url;
      },

      send: async ({ title, details, identity, hasScreenshot }) => {
        const token = get().token;
        if (token === "") {
          const body = get().composeBody(details, identity, hasScreenshot);
          return { kind: "browser", url: get().composeBrowserUrl(title, body) };
        }
        set({ sending: true });
        const body = get().composeBody(details, identity, hasScreenshot);
        const post = async (labels: string[]): Promise<Response> =>
          fetch(ISSUES_API_URL, {
            method: "POST",
            headers: {
              Authorization: `Bearer ${token}`,
              Accept: "application/vnd.github+json",
              "X-GitHub-Api-Version": "2022-11-28",
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              title,
              body,
              ...(labels.length > 0 ? { labels } : {}),
            }),
          });
        try {
          let response = await post(["feedback"]);
          if (response.status === 422) {
            // The label may not exist on the repo yet — the report still
            // ships; the retry carries no labels rather than dying here.
            response = await post([]);
          }
          if (response.status === 401) {
            set({ login: null, signIn: "invalid" });
            return {
              kind: "error",
              note: "GitHub rejected the saved token. Sign in again in Settings — your report is still here.",
            };
          }
          if (response.status === 403) {
            return {
              kind: "error",
              note: "GitHub refused the request (rate limit or missing issue permission). The report is still here — try again shortly, or send it via browser instead.",
            };
          }
          if (!response.ok) {
            return {
              kind: "error",
              note: `GitHub answered ${response.status}. The report is still here — nothing was lost; try again or send it via browser instead.`,
            };
          }
          const created = (await response.json()) as {
            html_url?: string;
            number?: number;
          };
          if (typeof created.html_url !== "string" || typeof created.number !== "number") {
            return {
              kind: "error",
              note: "GitHub's answer did not name the new issue. Check the repository's issues list to confirm whether it arrived.",
            };
          }
          return { kind: "created", url: created.html_url, issueNumber: created.number };
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          return {
            kind: "error",
            note: `The report could not leave the panel (${message}). Nothing was lost — try again, or send it via browser instead.`,
          };
        } finally {
          set({ sending: false });
        }
      },
    }),
    {
      name: "zaminpanel.feedback",
      storage: createJSONStorage(() => localStorage),
      // Only the token rides across boots; sign-in state and send outcomes
      // are runtime facts, recomputed honestly each session.
      partialize: (state) => ({ token: state.token }),
    },
  ),
);
