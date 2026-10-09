// The feedback page (ADR-0028): the honest route line, the paste → preview
// → remove loop, the send routes through the injected bridge, and the
// never-leak rule for the token. No desktop host, no real network.

import { fireEvent, render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FeedbackPage, setFeedbackBridgeForTests } from "./FeedbackPage";
import { useFeedback } from "../state/feedback";
import type { FeedbackBridge } from "./FeedbackPage";

vi.mock("../state/actions", () => ({}));

const pngBytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

function fakeBridge(over: Partial<FeedbackBridge> = {}): FeedbackBridge & {
  openedUrls: string[];
  copied: Uint8Array[];
} {
  const openedUrls: string[] = [];
  const copied: Uint8Array[] = [];
  return {
    openedUrls,
    copied,
    async openUrl(url: string) {
      openedUrls.push(url);
      return { ok: true as const };
    },
    async copyImage(bytes: Uint8Array) {
      copied.push(bytes);
      return { ok: true as const };
    },
    ...over,
  };
}

function pasteImage(target: Element) {
  const file = new File([pngBytes], "shot.png", { type: "image/png" });
  // jsdom's File predates arrayBuffer(); the page reads the pasted bytes
  // through it, so the test hands back the bytes it just put in.
  file.arrayBuffer = async () => pngBytes.slice().buffer;
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", {
    value: {
      items: [
        {
          type: "image/png",
          getAsFile: () => file,
        },
      ],
    },
  });
  fireEvent(target, event);
}

let mockUrlCounter = 0;

beforeEach(() => {
  localStorage.clear();
  useFeedback.setState({ token: "", login: null, signIn: "unknown", sending: false });
  setFeedbackBridgeForTests(null);
  // jsdom predates createObjectURL; the preview needs it. A tiny fake is
  // enough — the page only reads the URL back into <img src> and revokes.
  (URL as unknown as Record<string, unknown>).createObjectURL = (_blob: Blob) => {
    mockUrlCounter += 1;
    return `blob:mock/${mockUrlCounter}`;
  };
  (URL as unknown as Record<string, unknown>).revokeObjectURL = () => {};
});

afterEach(() => {
  delete (URL as unknown as Record<string, unknown>).createObjectURL;
  delete (URL as unknown as Record<string, unknown>).revokeObjectURL;
});

afterEach(cleanup);

describe("FeedbackPage", () => {
  it("states the browser route honestly when no token is saved", () => {
    render(<FeedbackPage />);
    expect(screen.getByText(/No GitHub token on this machine/)).toBeTruthy();
    expect(screen.getByText(/Send report/)).toBeTruthy();
  });

  it("names the signed-in account when the token proves out", () => {
    useFeedback.setState({ token: "good", login: "abasing", signIn: "signed-in" });
    render(<FeedbackPage />);
    expect(screen.getByText(/Signed in as abasing/)).toBeTruthy();
  });

  it("shows an invalid token's honest line", () => {
    useFeedback.setState({ token: "rotten", login: null, signIn: "invalid" });
    render(<FeedbackPage />);
    expect(screen.getByText(/was rejected/)).toBeTruthy();
  });

  it("keeps Send honest: no title, no send", () => {
    render(<FeedbackPage />);
    const send = screen.getByText("Send report").closest("button") as HTMLButtonElement;
    expect(send.disabled).toBe(true);
  });

  it("takes a pasted screenshot, previews it, and removes it on demand", async () => {
    render(<FeedbackPage />);
    const details = screen.getByLabelText("Details");
    pasteImage(details);
    await waitFor(() => expect(screen.getByAltText(/pasted screenshot/)).toBeTruthy());
    fireEvent.click(screen.getByText("Remove"));
    expect(screen.queryByAltText(/pasted screenshot/)).toBeNull();
  });

  it("sends via the browser when no token exists: opens the prefilled URL", async () => {
    const bridge = fakeBridge();
    setFeedbackBridgeForTests(bridge);
    render(<FeedbackPage />);
    fireEvent.change(screen.getByLabelText("Title"), {
      target: { value: "Bookmarks lost" },
    });
    fireEvent.change(screen.getByLabelText("Details"), {
      target: { value: "After a restart the bar is empty." },
    });
    fireEvent.click(screen.getByText("Send report"));
    await waitFor(() => expect(bridge.openedUrls.length).toBe(1));
    const url = new URL(bridge.openedUrls[0]!);
    expect(url.searchParams.get("title")).toBe("Bookmarks lost");
    // No screenshot was pasted: nothing rode the clipboard.
    expect(bridge.copied.length).toBe(0);
    expect(screen.getByText(/press Send on GitHub/)).toBeTruthy();
  });

  it("hands a pasted screenshot to the clipboard on the browser route", async () => {
    const bridge = fakeBridge();
    setFeedbackBridgeForTests(bridge);
    render(<FeedbackPage />);
    const details = screen.getByLabelText("Details");
    pasteImage(details);
    await waitFor(() => expect(screen.getByAltText(/pasted screenshot/)).toBeTruthy());
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Visual bug" } });
    fireEvent.change(details, { target: { value: "The strip overlaps." } });
    fireEvent.click(screen.getByText("Send report"));
    await waitFor(() => expect(bridge.copied.length).toBe(1));
    expect(Array.from(bridge.copied[0]!)).toEqual(Array.from(pngBytes));
    expect(screen.getByText(/copied back to your clipboard/i)).toBeTruthy();
  });

  it("files directly with a proven token and opens the created issue", async () => {
    const bridge = fakeBridge();
    setFeedbackBridgeForTests(bridge);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        jsonResponse(201, {
          html_url: "https://github.com/ZaminMC/ZIM/issues/12",
          number: 12,
        }),
      ),
    );
    useFeedback.setState({ token: "tok", login: "abasing", signIn: "signed-in" });
    render(<FeedbackPage />);
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Crash" } });
    fireEvent.change(screen.getByLabelText("Details"), { target: { value: "It died." } });
    fireEvent.click(screen.getByText("Send report"));
    await waitFor(() => expect(screen.getByText(/Filed as issue #12/)).toBeTruthy());
    expect(bridge.openedUrls[0]).toBe("https://github.com/ZaminMC/ZIM/issues/12");
    vi.unstubAllGlobals();
  });
});

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    json: async () => body,
  } as unknown as Response;
}
