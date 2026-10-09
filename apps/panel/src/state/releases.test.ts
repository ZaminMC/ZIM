// The releases state's read side (ADR-0029): parsing is tolerant, the
// rolling anchor is never part of the version history, and the failure
// sentences name what actually happened (unreachable, budget spent,
// HTTP status, wrong shape).

import { afterEach, describe, expect, it, vi } from "vitest";
import { listPublishedReleases } from "./releases";

function fetchOk(body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), { status: 200 }),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("listPublishedReleases", () => {
  it("parses the channel's releases and keeps the anchor out of the history", async () => {
    vi.stubGlobal(
      "fetch",
      fetchOk([
        {
          tag_name: "dev",
          name: "Update manifest — development channel",
          prerelease: true,
          html_url: "https://github.com/ZaminMC/ZIM/releases/tag/dev",
          published_at: "2026-10-08T17:00:00Z",
          assets: [],
        },
        {
          tag_name: "v0.2.0",
          name: "ZIM v0.2.0 — development pre-release",
          prerelease: true,
          html_url: "https://github.com/ZaminMC/ZIM/releases/tag/v0.2.0",
          published_at: "2026-10-08T16:00:00Z",
          assets: [
            {
              name: "ZIM_0.2.0_x64-setup.exe",
              browser_download_url: "https://example.com/setup.exe",
              size: 21_234_567,
            },
            { name: "broken-asset", browser_download_url: null },
          ],
        },
      ]),
    );
    const releases = await listPublishedReleases();
    expect(releases).toHaveLength(1);
    expect(releases[0]?.tagName).toBe("v0.2.0");
    expect(releases[0]?.assets).toHaveLength(1);
    expect(releases[0]?.assets[0]?.browserDownloadUrl).toBe("https://example.com/setup.exe");
  });

  it("a spent anonymous budget is the honest sentence, not a network lie", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response("rate limited", { status: 403 })),
    );
    await expect(listPublishedReleases()).rejects.toThrow(/budget is spent/);
  });

  it("an unreachable channel and a wrong shape are named sentences", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));
    await expect(listPublishedReleases()).rejects.toThrow(/unreachable/);

    vi.stubGlobal("fetch", fetchOk({ message: "not an array" }));
    await expect(listPublishedReleases()).rejects.toThrow(/unexpected shape/);
  });
});
