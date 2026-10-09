// The missing page's tests (ADR-0015, §58): an unknown internal URL is a
// real page in the tab — honest about what was not found, pointing at the
// scheme and the fleet — never a silent redirect, never a fake search.

import { render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { MissingPage } from "./MissingPage";

afterEach(cleanup);

describe("MissingPage", () => {
  it("names the URL that does not exist", () => {
    render(<MissingPage url="zim://nope/" />);
    expect(screen.getByRole("heading", { name: "No such page" })).toBeTruthy();
    expect(screen.getByText(/is not a ZIM page/)).toBeTruthy();
    expect(screen.getByText("zim://nope/")).toBeTruthy();
  });

  it("teaches the scheme and names the fleet page as the way back", () => {
    render(<MissingPage url="zim://typo" />);
    expect(screen.getByText(/Internal pages live under/)).toBeTruthy();
    expect(screen.getByText("zim://servers/")).toBeTruthy();
  });
});
