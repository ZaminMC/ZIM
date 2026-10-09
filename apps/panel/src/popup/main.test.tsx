// The popup overlay's boot law: the transparent child webview mounts
// PopupApp into the host's #root, and a missing #root fails loudly —
// an overlay that silently renders nothing would strand every menu.

import { describe, expect, it, vi, beforeEach } from "vitest";

const renderSpy = vi.fn();
const createRootSpy = vi.fn(() => ({ render: renderSpy, unmount: vi.fn() }));

vi.mock("react-dom/client", () => ({
  createRoot: createRootSpy,
}));
vi.mock("react", () => ({
  StrictMode: ({ children }: { children: never }) => children,
}));
vi.mock("./PopupApp", () => ({ PopupApp: () => null }));

beforeEach(() => {
  vi.resetModules();
  renderSpy.mockClear();
  createRootSpy.mockClear();
  document.body.innerHTML = "";
});

describe("popup boot", () => {
  it("mounts PopupApp into #root", async () => {
    document.body.innerHTML = "<div id='root'></div>";
    await import("./main");
    expect(createRootSpy).toHaveBeenCalledTimes(1);
    expect(createRootSpy).toHaveBeenCalledWith(document.getElementById("root"));
    expect(renderSpy).toHaveBeenCalledTimes(1);
    const [element] = renderSpy.mock.calls[0]!;
    expect(element.props.children.type.name).toBe("PopupApp");
  });

  it("a missing #root is a loud error naming the html file", async () => {
    await expect(import("./main")).rejects.toThrow("#root is missing from popup.html");
    expect(renderSpy).not.toHaveBeenCalled();
  });
});
