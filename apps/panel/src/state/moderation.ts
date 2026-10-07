// Moderation: the Players tab's shortcuts compose ordinary console lines
// and hand them to the same stdin path the Console uses. The server is
// the executor; the panel only writes the line — no new protocol concept,
// and a moderation action is indistinguishable from an operator's typing
// (events, logs, and the audit tell exactly that story). The honest limits
// come with the design: the panel cannot know whether the server accepted
// the command — the console's answer lands in the log, and the UI says so.

export type ModerationVerb = "kick" | "ban" | "op" | "deop" | "whitelist";

export interface ModerationVerbSpec {
  verb: ModerationVerb;
  /** The button label — short, because it sits in a per-player cluster. */
  label: string;
  /** The honest tooltip: what the line does on a vanilla-family server. */
  title: string;
  /** Ban removes a player until explicitly pardoned — it confirms first. */
  danger?: boolean;
}

/** The verbs the panel offers, in the order the cluster renders them. */
export const MODERATION_VERBS: ModerationVerbSpec[] = [
  {
    verb: "kick",
    label: "Kick",
    title: "Kick: disconnect the player now; they may rejoin",
  },
  {
    verb: "op",
    label: "Op",
    title: "Op: grant operator (server command) privileges",
  },
  {
    verb: "deop",
    label: "Deop",
    title: "Deop: revoke operator privileges",
  },
  {
    verb: "whitelist",
    label: "Whitelist",
    title: "Whitelist add: allow this name to join a whitelisted server",
  },
  {
    verb: "ban",
    label: "Ban",
    title: "Ban: refuse this name until it is pardoned",
    danger: true,
  },
];

/**
 * Vanilla username charset — the same rule the core log roster applies
 * ([A-Za-z0-9_]{1,16}). The rosters draw names from the server's own
 * answers, so a legal name is the norm; the guard exists so a modified
 * or hostile source can never talk the panel into composing a line with
 * spaces or newlines in it (a name is one argv word, never two commands).
 */
const USERNAME = /^[A-Za-z0-9_]{1,16}$/;

export function isLegalUsername(name: string): boolean {
  return USERNAME.test(name);
}

/**
 * The console line a verb composes, or null when the name is not a legal
 * username — the caller refuses honestly instead of sending.
 */
export function moderationLine(verb: ModerationVerb, name: string): string | null {
  if (!isLegalUsername(name)) return null;
  switch (verb) {
    case "kick":
      return `kick ${name}`;
    case "ban":
      return `ban ${name}`;
    case "op":
      return `op ${name}`;
    case "deop":
      return `deop ${name}`;
    case "whitelist":
      return `whitelist add ${name}`;
  }
}
