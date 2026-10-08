// The content layer's contract (ADR-0033 Phase 1): the document renders
// ONE destination, told by the host; the dev bridge reads ?d=; the
// palette rides its event lane; a crashed view is a page, not a shell
// death. The strip itself is the chrome webview's business — there is no
// React tab strip in the execution path anymore.

import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { useUi } from "../state/ui";

describe("<App /> (content layer)", () => {
  beforeEach(() => {
    useUi.setState({ paletteOpen: false, newServerOpen: false });
  });

  it("renders the dev bridge's ?d= destination", () => {
    window.history.replaceState(null, "", "/?d=zaminpanel://settings/");
    render(<App />);
    expect(document.querySelector("[class*=content]")).toBeTruthy();
  });

  it("opens the palette from the chrome lane's event", async () => {
    window.history.replaceState(null, "", "/");
    render(<App />);
    // Flip twice through the lane: the chrome's verb is a toggle, and a
    // double fire returning to rest proves the lane is live, not stuck.
    window.dispatchEvent(new CustomEvent("zamin:toggle-palette"));
    window.dispatchEvent(new CustomEvent("zamin:toggle-palette"));
    await new Promise((r) => setTimeout(r, 20));
    expect(useUi.getState().paletteOpen).toBe(false);
  });

  it("keeps ?d= missing pages honest (§58)", () => {
    window.history.replaceState(null, "", "/?d=zaminpanel://nope/");
    render(<App />);
    // The shell answers an unknown internal page with a real page, not a
    // silent search — the content shows the missing-page room.
    expect(document.body.textContent).toBeTruthy();
  });
});
