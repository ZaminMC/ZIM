// Error translation: protocol errors render with code + remediation so the
// UI can offer a next step, never a bare "Failed" (ARCHITECTURE-REVIEW §2.6).

import { ProtocolRequestError } from "../protocol/client";

export interface DescribedError {
  title: string;
  code?: string;
  remediation: string[];
  /** The typed error's structured context (e.g. PLUGIN_EXISTS's file). */
  context?: Record<string, unknown>;
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
  if (error instanceof Error) {
    return { title: error.message, remediation: [] };
  }
  return { title: "Something went wrong talking to the daemon.", remediation: [] };
}

/** Lifecycle verbs the UI may dispatch, with their protocol names. */
export const LIFECYCLE_VERBS = ["start", "stop", "restart", "kill"] as const;
export type LifecycleVerb = (typeof LIFECYCLE_VERBS)[number];
