// The frame webview's boot law: the shell's view process mounts into the
// host-provided #root and nowhere else, and a missing #root is a loud
// error — never a silent blank window the operator would have to debug.

import { describe, expect, it, vi, beforeEach } from "vitest";

const renderSpy = vi.fn();
const createRootSpy = vi.fn(() => ({ render: renderSpy, unmount: vi.fn() }));

vi.mock("react-dom/client", () => ({
  createRoot: createRootSpy,
}));
vi.mock("react", () => ({
  StrictMode: ({ children }: { children: never }) => children,
}));
vi.mock("./FrameApp", () => ({ FrameApp: () => null }));

beforeEach(() => {
  vi.resetModules();
  renderSpy.mockClear();
  createRootSpy.mockClear();
  document.body.innerHTML = "";
});

describe("frame boot", () => {
  it("mounts FrameApp into #root", async () => {
    document.body.innerHTML = "<div id='root'></div>";
    await import("./main");
    expect(createRootSpy).toHaveBeenCalledTimes(1);
    expect(createRootSpy).toHaveBeenCalledWith(document.getElementById("root"));
    expect(renderSpy).toHaveBeenCalledTimes(1);
    const [element] = renderSpy.mock.calls[0]!;
    expect(element.props.children.type.name).toBe("FrameApp");
  });

  it("a missing #root is a loud error naming the html file", async () => {
    await expect(import("./main")).rejects.toThrow("#root is missing from frame.html");
    expect(renderSpy).not.toHaveBeenCalled();
  });
});
