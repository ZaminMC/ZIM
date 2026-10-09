// Error translation: protocol errors render with code + remediation so the
// UI can offer a next step, never a bare "Failed" (ARCHITECTURE-REVIEW §2.6).
//
// The client's own failure classes translate too: protocol method names,
// daemon internals, and IPC wording never surface as the title. The
// technical detail lives in the remediation lines and the context — where
// a developer view can find it, and normal UI can ignore it.

import {
  ConnectionLostError,
  DisposedError,
  ProtocolRequestError,
  RequestTimeoutError,
} from "../protocol/client";

export interface DescribedError {
  title: string;
  code?: string;
  remediation: string[];
  /** The typed error's structured context (e.g. PLUGIN_EXISTS's file). */
  context?: Record<string, unknown>;
}

/** The method families a timeout can name, with the human request each
 *  one stands for. Anything unmapped falls to the generic title. */
const TIMEOUT_TITLES: [RegExp, string][] = [
  [/^catalog\./, "Unable to load the server catalog."],
  [/^plugins\./, "Unable to reach the plugin catalog."],
  [/^(server\.discover|discovery\.)/, "The scan could not finish."],
  [/^backups?\./, "The backup operation did not finish."],
  [/^jobs\./, "Unable to load the jobs."],
  [/^java\./, "The Java runtime operation did not finish."],
  [/^logs\./, "Unable to load the log history."],
  [/^metrics\./, "Unable to load the metrics history."],
  [/^players\./, "Unable to load the player list."],
];

function timeoutTitle(method: string): string {
  for (const [pattern, title] of TIMEOUT_TITLES) {
    if (pattern.test(method)) return title;
  }
  return "The request took too long.";
}

export function describeError(error: unknown): DescribedError {
  if (error instanceof ProtocolRequestError) {
    return {
      title: error.error.message,
      code: error.error.code,
      remediation: error.error.remediation ?? [],
      context: error.error.context,
    };
  }
  if (error instanceof RequestTimeoutError) {
    return {
      title: timeoutTitle(error.method),
      remediation: [
        "ZIM may be busy — wait a moment and try again.",
        `Details: no reply to ${error.method} within ${error.ms} ms.`,
      ],
      context: { method: error.method, timeoutMs: error.ms },
    };
  }
  if (error instanceof ConnectionLostError) {
    return {
      title: "The connection to ZIM dropped.",
      remediation: [
        "ZIM reconnects on its own — no restart needed. Try again in a moment.",
      ],
    };
  }
  if (error instanceof DisposedError) {
    return {
      title: "ZIM is shutting down.",
      remediation: [],
    };
  }
  if (error instanceof Error) {
    return { title: error.message, remediation: [] };
  }
  return { title: "Something went wrong. Try again in a moment.", remediation: [] };
}

/** Lifecycle verbs the UI may dispatch, with their protocol names. */
export const LIFECYCLE_VERBS = ["start", "stop", "restart", "kill"] as const;
export type LifecycleVerb = (typeof LIFECYCLE_VERBS)[number];
