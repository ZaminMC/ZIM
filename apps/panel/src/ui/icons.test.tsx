// The icon system's whole-surface law: EVERY export renders as an
// <svg> at the asked size, decorative by default (aria-hidden, no
// words — the microcopy rule means an icon carries no text), and every
// glyph accepts the panel's props. One loop over the module namespace
// keeps the 55 marks honest as the set grows: a new export that cannot
// render fails here, not in a view three screens deep.

import { render, cleanup } from "@testing-library/react";
import type { JSX } from "react";
import { afterEach, describe, expect, it } from "vitest";
import * as icons from "./icons";
import { IconZim } from "./icons";

type IconComponent = (p: { size?: number }) => JSX.Element;
const catalog = icons as unknown as Record<string, IconComponent>;

afterEach(cleanup);

const names = Object.keys(icons).filter((k) => k.startsWith("Icon"));

describe("the icon system", () => {
  it("holds the vocabulary — every export is an Icon* component", () => {
    expect(names.length).toBeGreaterThanOrEqual(50);
    for (const name of Object.keys(icons)) {
      expect(name, `${name} should start with Icon`).toMatch(/^Icon[A-Z]/);
      const value = (icons as Record<string, unknown>)[name];
      expect(typeof value, `${name} should be a component`).toBe("function");
    }
  });

  it.each(names)("%s renders an svg at the default 16", (name) => {
    const Icon = catalog[name]!;
    const { container } = render(<Icon />);
    const svg = container.firstElementChild;
    expect(svg?.tagName).toBe("svg");
    expect(svg?.getAttribute("width")).toBe("16");
    expect(svg?.getAttribute("height")).toBe("16");
  });

  it.each(names)("%s honors an explicit size and stays wordless", (name) => {
    const Icon = catalog[name]!;
    const { container } = render(<Icon size={24} />);
    const svg = container.firstElementChild as Element;
    expect(svg.getAttribute("width")).toBe("24");
    expect(svg.textContent ?? "").not.toMatch(/[A-Za-z]{2,}/);
  });

  it("the house mark renders by its own hand — 16 viewBox, stroke color rule", () => {
    const { container } = render(<IconZim size={20} />);
    const svg = container.firstElementChild as Element;
    expect(svg.tagName).toBe("svg");
    expect(svg.getAttribute("viewBox")).toBe("0 0 16 16");
    expect(svg.getAttribute("stroke")).toBe("currentColor");
    expect(svg.getAttribute("width")).toBe("20");
  });
});
