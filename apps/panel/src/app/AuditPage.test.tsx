// The audit page (§72 read side, ADR-0026): the trail renders newest
// first with the outcome the daemon gave, paging walks backward honestly,
// malformed lines are counted and said, and a read is never audited.

import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuditPage } from "./AuditPage";
import { ProtocolRequestError } from "../protocol/client";
import type { AuditEntry } from "../protocol/types";

vi.mock("../state/actions", () => ({
  listAudit: vi.fn(),
}));

import { listAudit } from "../state/actions";

const listAuditMock = listAudit as ReturnType<typeof vi.fn>;

function entryOf(method: string, extra: Partial<AuditEntry> = {}): AuditEntry {
  return {
    tsMs: 1_730_803_200_000,
    method,
    outcome: "ok",
    ...extra,
  };
}

beforeEach(() => {
  localStorage.clear();
  listAuditMock.mockReset().mockResolvedValue({ entries: [], hasMore: false, malformed: 0 });
});

afterEach(cleanup);

describe("AuditPage", () => {
  it("renders the trail newest-first with outcomes and clients", async () => {
    listAuditMock.mockResolvedValue({
      entries: [
        entryOf("server.start", {
          serverId: "demo",
          client: { name: "zim", version: "0.1.0" },
        }),
        entryOf("backup.restore", {
          outcome: "BACKUP_NOT_FOUND",
          client: { name: "zamin", version: "0.1.0" },
        }),
      ],
      hasMore: false,
      malformed: 0,
    });
    render(<AuditPage />);
    const rows = await screen.findAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.textContent).toContain("server.start");
    expect(rows[0]!.textContent).toContain("ok");
    expect(rows[1]!.textContent).toContain("BACKUP_NOT_FOUND");
    expect(rows[1]!.textContent).toContain("zamin v0.1.0");
  });

  it("the empty state admits the trail is empty; the read refusal is typed", async () => {
    render(<AuditPage />);
    await screen.findByText(/Nothing audited yet/);

    listAuditMock.mockRejectedValue(
      new ProtocolRequestError({
        code: "PROTOCOL_UNAVAILABLE",
        message: "The daemon is not connected",
      }),
    );
    cleanup();
    render(<AuditPage />);
    await screen.findByText("The daemon is not connected");
    expect(screen.queryByText(/Nothing audited yet/)).toBeNull();
  });

  it("paging asks for older entries by offset and appends them", async () => {
    const page1 = Array.from({ length: 100 }, (_, i) => entryOf(`op.new.${i}`));
    const page2 = [entryOf("op.old")];
    listAuditMock
      .mockResolvedValueOnce({ entries: page1, hasMore: true, malformed: 0 })
      .mockResolvedValueOnce({ entries: page2, hasMore: false, malformed: 0 });
    render(<AuditPage />);
    const button = await screen.findByRole("button", { name: "Load older entries" });
    fireEvent.click(button);
    await waitFor(() => expect(listAuditMock).toHaveBeenLastCalledWith({ limit: 100, offset: 100 }));
    await waitFor(() => expect(screen.getAllByRole("listitem")).toHaveLength(101));
    // The older page leaves no "load older" behind — the trail is done.
    expect(screen.queryByRole("button", { name: "Load older entries" })).toBeNull();
  });

  it("malformed lines are counted in plain words, never hidden", async () => {
    listAuditMock.mockResolvedValue({
      entries: [entryOf("server.stop")],
      hasMore: false,
      malformed: 2,
    });
    render(<AuditPage />);
    await screen.findByText(/2 lines on disk did not parse as audit JSON/);
  });

  it("the refresh verb re-reads page zero", async () => {
    listAuditMock.mockResolvedValue({ entries: [entryOf("op.one")], hasMore: false, malformed: 0 });
    render(<AuditPage />);
    await screen.findByText("op.one");
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(listAuditMock).toHaveBeenLastCalledWith({ limit: 100, offset: 0 }));
  });
});
