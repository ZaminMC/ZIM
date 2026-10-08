// The release channel's read side (ADR-0029): the versioned pre-releases
// the panel ships in, read straight from the main repo's public releases API.
// No token rides along (the repo is public; the anonymous budget is the
// honest cost), the installed version is marked where the host answered
// for it, and every failure — rate limit, network, shape — is a typed
// note, never a fake list.

import { describeError } from "./errors";

export interface PublishedRelease {
  tagName: string;
  name: string;
  publishedAt: string | null;
  prerelease: boolean;
  htmlUrl: string;
  assets: Array<{ name: string; browserDownloadUrl: string; size: number }>;
}

const RELEASES_API =
  "https://api.github.com/repos/ZaminMC/ZaminPanel/releases?per_page=10";

function parseRelease(raw: unknown): PublishedRelease | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.tag_name !== "string") return null;
  const assets = Array.isArray(r.assets) ? r.assets : [];
  return {
    tagName: r.tag_name,
    name: typeof r.name === "string" ? r.name : r.tag_name,
    publishedAt: typeof r.published_at === "string" ? r.published_at : null,
    prerelease: r.prerelease === true,
    htmlUrl: typeof r.html_url === "string" ? r.html_url : "",
    assets: assets.flatMap((a) => {
      if (typeof a !== "object" || a === null) return [];
      const asset = a as Record<string, unknown>;
      if (
        typeof asset.name !== "string" ||
        typeof asset.browser_download_url !== "string"
      ) {
        return [];
      }
      return [
        {
          name: asset.name,
          browserDownloadUrl: asset.browser_download_url,
          size: typeof asset.size === "number" ? asset.size : 0,
        },
      ];
    }),
  };
}

export async function listPublishedReleases(): Promise<PublishedRelease[]> {
  let response: Response;
  try {
    response = await fetch(RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" },
    });
  } catch {
    throw new Error(
      "The releases channel is unreachable from this machine — check the connection and try again.",
    );
  }
  if (response.status === 403) {
    // GitHub's anonymous budget is per IP and per hour; the honest note
    // says so instead of pretending the channel vanished.
    throw new Error(
      "GitHub's anonymous API budget is spent for this hour — the list will come back on its own; try again later.",
    );
  }
  if (!response.ok) {
    throw new Error(
      `The releases channel answered HTTP ${response.status} — try again later.`,
    );
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new Error("The releases channel answered something that is not JSON.");
  }
  if (!Array.isArray(body)) {
    throw new Error("The releases channel answered with an unexpected shape.");
  }
  const releases = body
    .map(parseRelease)
    .filter((r): r is PublishedRelease => r !== null);
  // The rolling manifest anchor is not a versioned release — keep it
  // out of the version history it exists to anchor.
  return releases.filter((r) => r.tagName !== "dev");
}

export function releasesSentence(error: unknown): string {
  return describeError(error).title;
}
