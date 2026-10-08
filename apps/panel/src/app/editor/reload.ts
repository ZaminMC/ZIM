// §35's reload knowledge: the editor knows how a saved file reaches the
// server, because the file's own position in the server's life tells it.
// The one unsafe verb — Bukkit's `/reload` — is deliberately absent: it
// leaks memory and half-applies configs, which is exactly the kind of
// pretending the founder forbids. The supported action is the restart,
// and the copy says so in one honest sentence.

export interface ReloadPlan {
  /** A restart may follow the save. The UI offers it behind a confirm. */
  restartable: boolean;
  /** One sentence about how the file actually loads. Absent for files
   *  with no story at all — no claim is the honest claim. */
  explain?: string;
}

/** The JVM-side configs: read at boot, and no live reload that works. */
const BOOT_CONFIG_PATTERN = /(^|\/)(server\.properties|bukkit\.yml|spigot\.yml|purpur\.yml|folia\.yml|paper-[a-z-]+\.yml|config\/paper-[a-z-]+\.yml|paper-world-defaults\.yml)$/;

export function reloadPlanFor(path: string): ReloadPlan {
  const lower = path.toLowerCase();
  if (BOOT_CONFIG_PATTERN.test(lower)) {
    return {
      restartable: true,
      explain: "This file is read at boot. Save & Restart applies it — a live reload is not offered because the JVM has none that works.",
    };
  }
  if (lower.endsWith(".yml") || lower.endsWith(".yaml") || lower.endsWith(".properties")) {
    return {
      restartable: true,
      explain: "Most plugins read their config at startup. The panel does not fake a plugin reload — Save & Restart applies it for real.",
    };
  }
  // A file with no story: the editor saves it and says nothing it
  // cannot know.
  return { restartable: false };
}
