// The about page (§58): the version is the host's answer, the daemon is
// the connection's, and the reserved rooms are named — never faked
// (§82). Nothing here invents a build number the host did not give.

import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { AboutPage } from "./AboutPage";
import { useConnection } from "../state/connection";
import { useUpdates } from "../state/updates";

beforeEach(() => {
  useUpdates.setState({ installedVersion: "0.1.4" });
  useConnection.setState({ status: "ready", daemon: { name: "zamind", version: "0.1.0" } });
});

afterEach(cleanup);

describe("AboutPage", () => {
  it("states the installed version the host answered for, and the daemon's", () => {
    render(<AboutPage />);
    expect(screen.getByText("0.1.4")).toBeTruthy();
    expect(screen.getByText(/zamind v0\.1\.0/)).toBeTruthy();
    expect(screen.getByText(/Development/)).toBeTruthy();
  });

  it("an unanswered host is said, not guessed", () => {
    useUpdates.setState({ installedVersion: null });
    render(<AboutPage />);
    expect(screen.getByText("the host has not answered yet")).toBeTruthy();
  });

  it("names the reserved rooms with their founder sections", () => {
    render(<AboutPage />);
    // Exact matches: the room names are their own <strong> elements, and
    // the address dialect rides a <code>.
    expect(screen.getByText("Dutchmen")).toBeTruthy();
    expect(screen.getByText("Extensions")).toBeTruthy();
    expect(screen.getByText("dutchmen:<id>")).toBeTruthy();
  });

  it("says where updates come from and who owns the restart", () => {
    render(<AboutPage />);
    expect(screen.getByText(/never restarts the session on its own/)).toBeTruthy();
  });
});
