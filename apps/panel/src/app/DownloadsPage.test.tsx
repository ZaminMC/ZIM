// The downloads page (§58's reserved URL, ADR-0029): the channel's
// version history as the API answered it — the anchor's rolling entry
// kept out of the version list, the installed build marked where the
// host answered for it, installers distinguished from portables, and
// every failure (rate limit, network, shape) a typed note.

import { render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DownloadsPage } from "./DownloadsPage";
import { useUpdates } from "../state/updates";
import type { PublishedRelease } from "../state/releases";

vi.mock("../state/releases", () => ({
  listPublishedReleases: vi.fn(),
}));

import { listPublishedReleases } from "../state/releases";
const listMock = listPublishedReleases as ReturnType<typeof vi.fn>;

function releaseOf(tag: string, over: Partial<PublishedRelease> = {}): PublishedRelease {
  return {
    tagName: tag,
    name: `ZIM ${tag} — development pre-release`,
    publishedAt: "2026-10-08T17:00:00Z",
    prerelease: true,
    htmlUrl: `https://github.com/ZaminMC/ZIM/releases/tag/${tag}`,
    assets: [
      {
        name: `ZIM_${tag.replace(/^v/, "")}_x64-setup.exe`,
        browserDownloadUrl: `https://example.com/${tag}-setup.exe`,
        size: 21_234_567,
      },
      {
        name: `ZIM-${tag.replace(/^v/, "")}-windows-x64.zip`,
        browserDownloadUrl: `https://example.com/${tag}-portable.zip`,
        size: 19_111_111,
      },
    ],
    ...over,
  };
}

beforeEach(() => {
  localStorage.clear();
  useUpdates.setState({ installedVersion: null });
  listMock.mockReset().mockResolvedValue([]);
});

afterEach(cleanup);

describe("DownloadsPage", () => {
  it("renders the channel's versions newest-first with their assets", async () => {
    listMock.mockResolvedValue([releaseOf("v0.2.0"), releaseOf("v0.1.7")]);
    render(<DownloadsPage />);
    await waitFor(() => expect(screen.getByText("v0.2.0")).toBeTruthy());
    expect(screen.getByText("v0.1.7")).toBeTruthy();
    const links = screen.getAllByRole("link", { name: /-setup\.exe/ });
    expect(links).toHaveLength(2);
    expect(links[0]?.getAttribute("href")).toBe("https://example.com/v0.2.0-setup.exe");
  });

  it("marks the installed build where the host answered for it", async () => {
    useUpdates.setState({ installedVersion: "0.2.0" });
    listMock.mockResolvedValue([releaseOf("v0.2.0"), releaseOf("v0.1.7")]);
    render(<DownloadsPage />);
    await waitFor(() => expect(screen.getAllByText("Installed")).toHaveLength(1));
  });

  it("says the page cannot overpromise: pre-release software, updater verifies", async () => {
    listMock.mockResolvedValue([releaseOf("v0.2.0")]);
    render(<DownloadsPage />);
    await waitFor(() => expect(screen.getByText(/pre-release software until v1\.0\.0/)).toBeTruthy());
    expect(screen.getByText(/verifies the minisign signature/)).toBeTruthy();
  });

  it("a spent anonymous budget is a typed note, not a fake empty list", async () => {
    listMock.mockRejectedValue(
      new Error("GitHub's anonymous API budget is spent for this hour — try again later."),
    );
    render(<DownloadsPage />);
    await waitFor(() => expect(screen.getByText(/budget is spent/)).toBeTruthy());
    expect(screen.queryByText(/No published releases/)).toBeNull();
  });

  it("an empty channel is said, not filled", async () => {
    render(<DownloadsPage />);
    await waitFor(() => expect(screen.getByText(/no published releases yet/)).toBeTruthy());
  });
});
