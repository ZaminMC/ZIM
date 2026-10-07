// Publish modal regressions (ADR-0017): the §42 chip lights up only when
// changes exist, the diff renders M/A/D marks, the security panel opens
// on a blocked publish with the four §45 verbs reachable, Exclude File
// saves the exclude rule, Review records the false positive and refreshes
// the preview, Publish Anyway needs the explicit second confirmation, and
// the §43 Dutchmen changelog room is named-and-disabled — never faked.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PublishModal, changedChipClass, parseRuleText, statusMark } from "./PublishModal";
import type { PublishPreviewResult, SecretFinding } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listPublishProviders: vi.fn(),
  getPublishConfig: vi.fn(),
  setPublishConfig: vi.fn(),
  previewPublish: vi.fn(),
  executePublish: vi.fn(),
  getPublishState: vi.fn(),
  setPublishReview: vi.fn(),
}));

import {
  executePublish,
  listPublishProviders,
  previewPublish,
  setPublishConfig,
  setPublishReview,
} from "../state/actions";

const previewMock = previewPublish as ReturnType<typeof vi.fn>;
const setConfigMock = setPublishConfig as ReturnType<typeof vi.fn>;
const executeMock = executePublish as ReturnType<typeof vi.fn>;
const reviewMock = setPublishReview as ReturnType<typeof vi.fn>;
const providersMock = listPublishProviders as ReturnType<typeof vi.fn>;

function finding(extra: Partial<SecretFinding> = {}): SecretFinding {
  return {
    file: "plugins/DiscordSRV/config.yml",
    line: 1,
    kind: "discord-bot-token",
    severity: "critical",
    excerpt: "MTE0…[redacted] (len 73)",
    detector: "token-pattern",
    reviewed: false,
    ...extra,
  };
}

function previewOf(extra: Partial<PublishPreviewResult> = {}): PublishPreviewResult {
  return {
    serverId: "demo",
    config: {
      selection: {
        includes: [{ kind: "folder", path: "plugins" }],
        excludes: [],
      },
      providerId: "archive",
      providerSettings: {},
      title: "Box Demo",
      description: "",
      version: "1.0.0",
      changelog: "",
    },
    files: [
      {
        path: "plugins/TAB/config.yml",
        status: "modified",
        size: 100,
        sha512: "aa",
      },
      {
        path: "plugins/example/config.yml",
        status: "added",
        size: 5,
        sha512: "bb",
      },
    ],
    counts: { added: 1, modified: 1, removed: 0, unchanged: 0, changed: 2 },
    scan: {
      findings: [finding()],
      filesScanned: 2,
      filesSkipped: 0,
    },
    blockingCount: 1,
    selectedFiles: 2,
    selectedBytes: 105,
    ...extra,
  };
}

beforeEach(() => {
  providersMock.mockReset().mockResolvedValue({
    providers: [
      { id: "archive", displayName: "Archive only (no upload)", needsCredential: false, settings: [] },
      {
        id: "local-dir",
        displayName: "Local folder",
        needsCredential: false,
        settings: [{ key: "outDir", description: "absolute folder" }],
      },
    ],
  });
  previewMock.mockReset().mockResolvedValue(previewOf());
  setConfigMock.mockReset().mockImplementation(
    (_serverId: string, config: PublishPreviewResult["config"]) =>
      Promise.resolve(config),
  );
  executeMock.mockReset().mockRejectedValue({
    name: "ProtocolRequestError",
    error: {
      code: "PUBLISH_SECRETS_DETECTED",
      message: "the security scan found 1 unreviewed finding(s)",
      context: { blockingCount: 1, files: [] },
      remediation: ["review", "exclude-file", "publish-anyway", "cancel"],
    },
  });
  reviewMock.mockReset().mockResolvedValue(previewOf());
});

afterEach(() => {
  cleanup();
});

describe("publish modal helpers", () => {
  it("parses and prints the CLI-compatible rule syntax", () => {
    expect(parseRuleText("folder:plugins/TAB")).toEqual({ kind: "folder", path: "plugins/TAB" });
    expect(parseRuleText("file:server.properties")).toEqual({
      kind: "file",
      path: "server.properties",
    });
    expect(parseRuleText("glob:plugins/**/*.yml")).toEqual({
      kind: "glob",
      pattern: "plugins/**/*.yml",
    });
    expect(parseRuleText("nonsense")).toBeNull();
    expect(parseRuleText("folder:")).toBeNull();
    expect(parseRuleText("mystery:x")).toBeNull();
  });

  it("marks the diff statuses the founder's way", () => {
    expect(statusMark("added")).toBe("A");
    expect(statusMark("modified")).toBe("M");
    expect(statusMark("removed")).toBe("D");
    expect(statusMark("unchanged")).toBe("·");
  });

  it("emphasizes the change chip only when changes exist (§42)", () => {
    expect(changedChipClass(0)).not.toBe(changedChipClass(12));
    expect(changedChipClass(0)).toContain("changedQuiet");
    expect(changedChipClass(12)).toContain("changedEmph");
  });
});

describe("publish modal flow", () => {
  it("renders the diff, the findings, and the blocked publish", async () => {
    render(<PublishModal serverId="demo" serverName="Box Demo" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("publish-modal")).toBeTruthy());

    const chip = screen.getByTestId("changed-chip");
    expect(chip.textContent).toBe("2 files changed");
    expect(screen.getByText("plugins/TAB/config.yml")).toBeTruthy();
    expect(screen.getByText("M")).toBeTruthy();
    expect(screen.getByText(/discord-bot-token/)).toBeTruthy();
    expect(screen.getByTestId("blocking-count").textContent).toContain("1 finding");

    // Publish routes into the security check, not a dead-end error.
    fireEvent.click(screen.getByTestId("publish-run"));
    await waitFor(() => expect(screen.getByTestId("security-check")).toBeTruthy());
    // The four §45 verbs are reachable: Exclude and Review per finding,
    // Publish Anyway (two-step), Cancel.
    expect(screen.getAllByText("Exclude file").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Review").length).toBeGreaterThan(0);
    expect(screen.getByTestId("publish-anyway-arm")).toBeTruthy();
    expect(screen.getAllByText("Cancel").length).toBeGreaterThan(0);
  });

  it("Exclude File saves the rule and Review records the false positive", async () => {
    previewMock.mockResolvedValue(previewOf());
    render(<PublishModal serverId="demo" serverName="Box Demo" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("publish-modal")).toBeTruthy());

    fireEvent.click(screen.getAllByText("Exclude file")[0] as HTMLElement);
    await waitFor(() => expect(setConfigMock).toHaveBeenCalled());
    const saved = setConfigMock.mock.calls[0]?.[1] as PublishPreviewResult["config"] | undefined;
    expect(saved?.selection.excludes).toEqual([
      { kind: "file", path: "plugins/DiscordSRV/config.yml" },
    ]);

    fireEvent.click(screen.getAllByText("Review")[0] as HTMLElement);
    await waitFor(() => expect(reviewMock).toHaveBeenCalled());
    expect(reviewMock.mock.calls[0]).toEqual([
      "demo",
      "plugins/DiscordSRV/config.yml",
      "discord-bot-token",
      true,
    ]);
  });

  it("Publish Anyway requires the explicit second confirmation", async () => {
    executeMock.mockResolvedValue({
      job: {
        jobId: "11111111-1111-1111-1111-111111111111",
        kind: "publish.execute",
        serverId: "demo",
        state: "running",
        createdAtMs: 0,
      },
    });
    render(<PublishModal serverId="demo" serverName="Box Demo" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("publish-modal")).toBeTruthy());
    fireEvent.click(screen.getByTestId("publish-run"));
    await waitFor(() => expect(screen.getByTestId("security-check")).toBeTruthy());

    // One click only ARMS the confirmation; the typed second click fires.
    fireEvent.click(screen.getByTestId("publish-anyway-arm"));
    expect(executeMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("publish-anyway-confirm"));
    await waitFor(() => expect(executeMock).toHaveBeenCalled());
    expect(executeMock.mock.calls[0]).toEqual(["demo", true]);
  });

  it("a clean preview publishes directly and the changelog room stays disabled", async () => {
    executeMock.mockResolvedValue({
      job: {
        jobId: "22222222-2222-2222-2222-222222222222",
        kind: "publish.execute",
        serverId: "demo",
        state: "running",
        createdAtMs: 0,
      },
    });
    previewMock.mockResolvedValue(
      previewOf({
        scan: { findings: [], filesScanned: 2, filesSkipped: 0 },
        blockingCount: 0,
      }),
    );
    render(<PublishModal serverId="demo" serverName="Box Demo" onClose={() => undefined} />);
    await waitFor(() => expect(screen.getByTestId("publish-modal")).toBeTruthy());

    expect(screen.queryByTestId("blocking-count")).toBeNull();
    const reserved = screen.getByText("Generate with Dutchmen — reserved");
    expect((reserved as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByTestId("publish-run"));
    await waitFor(() => expect(executeMock).toHaveBeenCalled());
    expect(executeMock.mock.calls[0]).toEqual(["demo", false]);
  });
});
