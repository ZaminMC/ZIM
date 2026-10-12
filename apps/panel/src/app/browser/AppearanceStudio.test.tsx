// The Appearance Studio's contract (the Arix theme port): the new tab's
// bottom-right door opens the sidebar, every control is live (a change
// re-states the CSS variables the token layer consumes), Escape and the
// scrim close it, and the reset returns the Arix defaults.

import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { AppearanceStudio } from "./AppearanceStudio";
import { THEME_DEFAULTS, THEME_KEY, useTheme } from "../../state/theme";
import { useUi } from "../../state/ui";

describe("<AppearanceStudio />", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useUi.getState().setStudioOpen(false);
    useTheme.setState({ theme: { ...THEME_DEFAULTS } });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("is closed until the ui store opens it, then shows its sections", async () => {
    const { container } = render(<AppearanceStudio />);
    // Closed: the sheet is translated off-canvas and inert.
    const sheet = container.querySelector("aside");
    expect(sheet).not.toBeNull();
    expect(sheet!.className).not.toMatch(/studioOpen/);

    useUi.getState().setStudioOpen(true);
    await waitFor(() => {
      expect(screen.getByText("Appearance")).toBeTruthy();
    });
    for (const section of ["Mode", "Glass", "Accent", "Corners", "Background"]) {
      expect(screen.getByText(section)).toBeTruthy();
    }
  });

  it("every control is live — a mode change re-states the document's variables", async () => {
    useUi.getState().setStudioOpen(true);
    render(<AppearanceStudio />);
    const light = await screen.findByRole("radio", { name: "Light" });
    fireEvent.click(light);
    expect(document.documentElement.dataset.mode).toBe("light");
    // The change saved — the same key the inline boot script reads.
    const saved = JSON.parse(window.localStorage.getItem(THEME_KEY) ?? "{}");
    expect(saved.mode).toBe("light");
  });

  it("the glass switch stands the translucency down through data-glass", async () => {
    useUi.getState().setStudioOpen(true);
    render(<AppearanceStudio />);
    const glassSwitch = await screen.findByRole("switch", { name: "Glass surfaces" });
    fireEvent.click(glassSwitch);
    expect(document.documentElement.dataset.glass).toBe("off");
  });

  it("Escape closes the studio", async () => {
    useUi.getState().setStudioOpen(true);
    render(<AppearanceStudio />);
    await screen.findByText("Appearance");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(useUi.getState().studioOpen).toBe(false);
  });

  it("the reset returns the Arix defaults", async () => {
    useTheme.setState({
      theme: { ...THEME_DEFAULTS, mode: "light", primary: "#0f9d6e" },
    });
    useUi.getState().setStudioOpen(true);
    render(<AppearanceStudio />);
    const reset = await screen.findByRole("button", { name: /Reset to defaults/ });
    fireEvent.click(reset);
    await waitFor(() => {
      expect(useTheme.getState().theme).toEqual(THEME_DEFAULTS);
    });
    expect(document.documentElement.dataset.mode).toBe("dark");
  });
});
