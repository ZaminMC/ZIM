// Connection profiles (ADR-0011) on their two faces. The pure face:
// validateDraft's order of refusals — a name, then an address, then the
// host:port shape, then the token, then the fingerprint's 64-hex law
// (colons are spelling, not content). The wired face: activation of the
// active profile is just a close, activation of another re-opens the
// wire with that token, and a submitted draft is normalized (trimmed,
// colon-stripped, lowercased fingerprint) before it ever becomes a
// profile.

import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectionsModal, validateDraft } from "./ConnectionsModal";
import { LOCAL_PROFILE, useConnections } from "../state/connections";
import { useUi } from "../state/ui";

vi.mock("../state/wire", () => ({ reconnectWire: vi.fn() }));
import { reconnectWire } from "../state/wire";
const reconnectMock = reconnectWire as ReturnType<typeof vi.fn>;

function draftOf(over: Partial<Parameters<typeof validateDraft>[0]>) {
  return { name: "box", addr: "203.0.113.7:7443", token: "t", fingerprint: "", ...over };
}

beforeEach(() => {
  localStorage.clear();
  useConnections.setState({ remotes: [], activeId: LOCAL_PROFILE.id });
  useUi.setState({ connectionsOpen: false });
  reconnectMock.mockReset();
});

afterEach(cleanup);

function remoteOf(name: string) {
  return { id: `remote-${name}`, name, addr: "10.0.0.2:7443", token: "t", fingerprint: "" };
}

describe("validateDraft", () => {
  it("accepts a complete draft", () => {
    expect(validateDraft(draftOf({ fingerprint: "ab".repeat(32) }))).toBeNull();
  });

  it("refuses in order: name, address, shape, token, fingerprint", () => {
    expect(validateDraft(draftOf({ name: "  " }))).toBe("A name is required.");
    expect(validateDraft(draftOf({ addr: " " }))).toBe("An address (host:port) is required.");
    expect(validateDraft(draftOf({ addr: "203.0.113.7" }))).toBe(
      "Use host:port, e.g. 203.0.113.7:7443.",
    );
    expect(validateDraft(draftOf({ addr: "host:" }))).toBe("Use host:port, e.g. 203.0.113.7:7443.");
    expect(validateDraft(draftOf({ token: "" }))).toBe("The agent's token is required.");
    expect(validateDraft(draftOf({ fingerprint: "nothex" }))).toBe(
      "The fingerprint is 64 hex characters (sha-256).",
    );
    expect(validateDraft(draftOf({ fingerprint: "ab".repeat(31) }))).toBe(
      "The fingerprint is 64 hex characters (sha-256).",
    );
  });

  it("an empty fingerprint is honest skip-verify, not an error", () => {
    expect(validateDraft(draftOf({ fingerprint: "" }))).toBeNull();
  });

  it("colon-separated and uppercase fingerprints are spelling, and pass", () => {
    // 64 hex characters, colon-grouped like the agent prints them.
    const withColons = (":" + "AB").repeat(32).slice(1);
    expect(withColons.split(":")).toHaveLength(32);
    expect(withColons.replace(/:/g, "").length).toBe(64);
    expect(validateDraft(draftOf({ fingerprint: withColons }))).toBeNull();
  });
});

describe("<ConnectionsModal />", () => {
  it("renders nothing while closed", () => {
    render(<ConnectionsModal />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("the local profile is always first, named as this machine", () => {
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    expect(screen.getByRole("dialog", { name: "Connections" })).toBeTruthy();
    expect(screen.getByText(LOCAL_PROFILE.name)).toBeTruthy();
    expect(screen.getByText("ZIM on this machine")).toBeTruthy();
  });

  it("remote rows carry their address and their pin state", () => {
    useConnections.setState({
      remotes: [
        { ...remoteOf("pinned"), fingerprint: "ab".repeat(32) },
        remoteOf("bare"),
      ],
    });
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    expect(screen.getByText("pinned")).toBeTruthy();
    expect(screen.getByText("10.0.0.2:7443 · pinned")).toBeTruthy();
    expect(screen.getByText("bare")).toBeTruthy();
    expect(screen.getByText("10.0.0.2:7443 · unpinned")).toBeTruthy();
  });

  it("activating the active profile is just a close — the wire is not touched", () => {
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByText("ZIM on this machine"));
    expect(useUi.getState().connectionsOpen).toBe(false);
    expect(reconnectMock).not.toHaveBeenCalled();
  });

  it("activating a remote re-opens the wire with that profile and closes", () => {
    const remote = remoteOf("Hetzner");
    useConnections.setState({ remotes: [remote] });
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByText("Hetzner"));
    expect(useConnections.getState().activeId).toBe(remote.id);
    expect(reconnectMock).toHaveBeenCalledTimes(1);
    expect(useUi.getState().connectionsOpen).toBe(false);
  });

  it("forgetting a remote removes it; forgetting the active one falls back to local", () => {
    const remote = remoteOf("box");
    useConnections.setState({ remotes: [remote], activeId: remote.id });
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByRole("button", { name: "Forget box" }));
    expect(useConnections.getState().remotes).toEqual([]);
    expect(useConnections.getState().activeId).toBe(LOCAL_PROFILE.id);
  });

  it("an empty submission is a visible error — nothing is added, the form stays", () => {
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByText("+ Add remote"));
    fireEvent.click(screen.getByText("Connect"));
    expect(screen.getByText("A name is required.")).toBeTruthy();
    expect(useConnections.getState().remotes).toEqual([]);
    expect(screen.getByLabelText("Name")).toBeTruthy(); // the form is still open
    expect(reconnectMock).not.toHaveBeenCalled();
  });

  it("a valid submission normalizes the draft, adds it, activates it, and re-opens the wire", () => {
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByText("+ Add remote"));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "  Hetzner box " } });
    fireEvent.change(screen.getByLabelText("Address"), {
      target: { value: " 203.0.113.7:7443 " },
    });
    fireEvent.change(screen.getByLabelText("Token"), { target: { value: " secret " } });
    const upper = (":" + "AB").repeat(32).slice(1);
    fireEvent.change(screen.getByLabelText("Certificate fingerprint"), {
      target: { value: upper },
    });
    fireEvent.click(screen.getByText("Connect"));
    const remotes = useConnections.getState().remotes;
    expect(remotes).toHaveLength(1);
    expect(remotes[0]!).toMatchObject({
      name: "Hetzner box",
      addr: "203.0.113.7:7443",
      token: "secret",
      fingerprint: "ab".repeat(32),
    });
    expect(useConnections.getState().activeId).toBe(remotes[0]!.id);
    expect(reconnectMock).toHaveBeenCalledTimes(1);
    expect(useUi.getState().connectionsOpen).toBe(false);
  });

  it("cancel returns to the list — the draft is not remembered", () => {
    useUi.setState({ connectionsOpen: true });
    render(<ConnectionsModal />);
    fireEvent.click(screen.getByText("+ Add remote"));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "ghost" } });
    fireEvent.click(screen.getByText("Cancel"));
    expect(screen.getByText("+ Add remote")).toBeTruthy();
    expect(useConnections.getState().remotes).toEqual([]);
  });
});
