// Connection profiles: defaults, CRUD, and the transport spec the wire
// builds from the active profile (ADR-0011).

import { beforeEach, describe, expect, it } from "vitest";
import {
  LOCAL_PROFILE,
  activeProfile,
  boxHostOf,
  boxHostOfAddr,
  hostSpec,
  transportSpec,
  useConnections,
} from "./connections";

function resetStore() {
  localStorage.clear();
  useConnections.setState({ remotes: [], activeId: LOCAL_PROFILE.id });
}

describe("connection profiles", () => {
  beforeEach(resetStore);

  it("defaults to the local profile with no remotes", () => {
    const state = useConnections.getState();
    expect(state.remotes).toEqual([]);
    expect(state.activeId).toBe("local");
    expect(activeProfile(state)).toEqual(LOCAL_PROFILE);
  });

  it("adds a remote, activates it, and reports it as active", () => {
    const id = useConnections.getState().addRemote({
      name: "Hetzner box",
      addr: "203.0.113.7:7443",
      token: "secret",
      fingerprint: "ab".repeat(32),
    });
    expect(id).not.toBe("local");
    useConnections.getState().setActive(id);
    const state = useConnections.getState();
    expect(state.activeId).toBe(id);
    const profile = activeProfile(state);
    expect("addr" in profile && profile.addr).toBe("203.0.113.7:7443");
  });

  it("removing the active remote falls back to local", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "10.0.0.2:7443",
      token: "t",
      fingerprint: "",
    });
    useConnections.getState().setActive(id);
    useConnections.getState().removeRemote(id);
    expect(useConnections.getState().activeId).toBe("local");
    expect(useConnections.getState().remotes).toEqual([]);
  });

  it("persists remotes and the active id to localStorage", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "10.0.0.2:7443",
      token: "t",
      fingerprint: "",
    });
    useConnections.getState().setActive(id);
    const raw = localStorage.getItem("zamin.connections");
    expect(raw).toBeTruthy();
    const parsed: unknown = JSON.parse(raw ?? "{}");
    expect((parsed as { state: { activeId: string } }).state.activeId).toBe(id);
    expect(
      (parsed as { state: { remotes: unknown[] } }).state.remotes,
    ).toHaveLength(1);
  });
});

describe("transportSpec", () => {
  beforeEach(resetStore);

  it("local profile: plain bridge url, no auth", () => {
    const spec = transportSpec("ws://127.0.0.1:8787", useConnections.getState());
    expect(spec).toEqual({ url: "ws://127.0.0.1:8787", auth: undefined });
  });

  it("remote profile: relay query with fingerprint and the token as auth", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "203.0.113.7:7443",
      token: "secret-token",
      fingerprint: "AB".repeat(32),
    });
    useConnections.getState().setActive(id);
    const state = useConnections.getState();
    const spec = transportSpec("ws://127.0.0.1:8787", state);
    expect(spec.auth).toBe("secret-token");
    expect(spec.url.startsWith("ws://127.0.0.1:8787/?")).toBe(true);
    expect(spec.url).toContain("remote=203.0.113.7%3A7443");
    // The fingerprint is normalized once at profile creation time — the
    // spec carries it verbatim.
    expect(spec.url).toContain("fingerprint=");
  });

  it("remote profile without a fingerprint omits the parameter (skip-verify)", () => {
    useConnections.getState().addRemote({
      name: "box",
      addr: "10.0.0.2:7443",
      token: "t",
      fingerprint: "",
    });
    const spec = transportSpec("ws://127.0.0.1:8787", useConnections.getState());
    expect(spec.url).not.toContain("fingerprint");
  });
});

describe("hostSpec (the Tauri path)", () => {
  beforeEach(resetStore);

  it("local profile: no remote, no auth — the host speaks the local socket", () => {
    expect(hostSpec(useConnections.getState())).toEqual({
      remote: null,
      auth: undefined,
    });
  });

  it("pinned remote profile: the fields the host command expects, token as auth", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "203.0.113.7:7443",
      token: "secret-token",
      fingerprint: "AB".repeat(32),
    });
    useConnections.getState().setActive(id);
    const spec = hostSpec(useConnections.getState());
    expect(spec.remote).toEqual({
      addr: "203.0.113.7:7443",
      token: "secret-token",
      fingerprint: "AB".repeat(32),
    });
    expect(spec.auth).toBe("secret-token");
  });

  it("unpinned remote profile: empty fingerprint passes through, the host owns the rule", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "10.0.0.2:7443",
      token: "t",
      fingerprint: "",
    });
    useConnections.getState().setActive(id);
    const spec = hostSpec(useConnections.getState());
    expect(spec.remote?.fingerprint).toBe("");
    expect(spec.auth).toBe("t");
  });

  it("an inactive remote is invisible to the spec", () => {
    useConnections.getState().addRemote({
      name: "box",
      addr: "10.0.0.2:7443",
      token: "t",
      fingerprint: "",
    });
    expect(hostSpec(useConnections.getState())).toEqual({
      remote: null,
      auth: undefined,
    });
  });
});

describe("boxHostOf (the join dialect's box host)", () => {
  beforeEach(resetStore);

  it("local profile: the box is localhost", () => {
    expect(boxHostOf(activeProfile(useConnections.getState()))).toBe("localhost");
  });

  it("remote profile: the addr's host part, port stripped", () => {
    const id = useConnections.getState().addRemote({
      name: "box",
      addr: "203.0.113.7:7443",
      token: "t",
      fingerprint: "",
    });
    useConnections.getState().setActive(id);
    expect(boxHostOf(activeProfile(useConnections.getState()))).toBe("203.0.113.7");
  });

  it("bracketed IPv6 addr: the host without the brackets", () => {
    expect(boxHostOfAddr("[::1]:7001")).toBe("::1");
    expect(boxHostOfAddr("[fe80::1]:7001")).toBe("fe80::1");
  });

  it("a malformed addr degrades to localhost — never an unmatchable hint", () => {
    expect(boxHostOfAddr("")).toBe("localhost");
    expect(boxHostOfAddr("no-port-host")).toBe("localhost");
    expect(boxHostOfAddr("[]:7001")).toBe("localhost");
    // A hostname with no port colon: the whole word IS the host when a
    // colon exists nowhere — but the agent addr always carries a port,
    // so a colon-less spelling is malformed, not a host.
    expect(boxHostOfAddr("box.local")).toBe("localhost");
  });
});
