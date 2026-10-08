// The error note (§81): the shared body under every alert. The founder's
// bar has three parts and each is load-bearing — the sentence, the "what
// to do next" when the protocol typed one, and the technical details
// behind [View details] (available, never forced). A bare local sentence
// shows no disclosure: there is nothing technical to reveal, and a
// summary that opens onto nothing would be a fake control (§82).

import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ErrorNote } from "./ErrorNote";

 describe("ErrorNote", () => {
   afterEach(cleanup);
  it("a bare sentence carries no remediation and no disclosure", () => {
    render(<ErrorNote error={{ title: "The port must be a whole number.", remediation: [] }} />);
    expect(screen.getByText("The port must be a whole number.")).toBeTruthy();
    expect(screen.queryByText("View details")).toBeNull();
  });

  it("typed remediation rides the sentence", () => {
    render(
      <ErrorNote
        error={{
          title: "The file is too large to copy.",
          remediation: ["Free some space on the server disk.", "Copy a narrower selection."],
        }}
      />,
    );
    expect(screen.getByText("The file is too large to copy.")).toBeTruthy();
    expect(screen.getByText("Free some space on the server disk.")).toBeTruthy();
    expect(screen.getByText("Copy a narrower selection.")).toBeTruthy();
    expect(screen.queryByText("View details")).toBeNull();
  });

  it("the code and the structured context hide behind [View details]", () => {
    render(
      <ErrorNote
        error={{
          title: "The plugin could not be installed.",
          code: "PLUGIN_INCOMPATIBLE",
          remediation: ["Install a build for this server version."],
          context: { file: "essentialsx.jar", reason: "made for 1.20, server runs 1.21" },
        }}
      />,
    );
    expect(screen.getByText("View details")).toBeTruthy();
    // Closed by default — the technical layer is one click, not a wall.
    expect(document.querySelector("details")?.open).toBe(false);
  });

  it("the disclosure opens onto the code and the context, verbatim", () => {
    render(
      <ErrorNote
        error={{
          title: "The plugin could not be installed.",
          code: "PLUGIN_INCOMPATIBLE",
          remediation: [],
          context: { file: "essentialsx.jar" },
        }}
      />,
    );
    screen.getByText("View details").click();
    const pre = document.querySelector("pre");
    expect(pre?.textContent).toContain("code: PLUGIN_INCOMPATIBLE");
    expect(pre?.textContent).toContain('"file": "essentialsx.jar"');
  });

  it("a code with an empty context still discloses — the code is the detail", () => {
    render(<ErrorNote error={{ title: "The server refused.", code: "FS_SYMLINK_REFUSED", remediation: [] }} />);
    expect(screen.getByText("View details")).toBeTruthy();
    screen.getByText("View details").click();
    expect(document.querySelector("pre")?.textContent).toBe("code: FS_SYMLINK_REFUSED");
  });
});
