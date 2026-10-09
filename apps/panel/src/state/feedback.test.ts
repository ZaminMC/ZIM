// The feedback lane's rules (ADR-0028), against a stubbed network: two
// honest routes (token POST / browser handoff), the token never leaking
// into a body, a URL, or an error sentence, the 422 label retry, and the
// typed failures that keep the operator's text alive.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ISSUES_API_URL,
  NEW_ISSUE_URL,
  composeBrowserUrlChecked,
  diagnosticsBlock,
  useFeedback,
  type FeedbackIdentity,
} from "./feedback";

const identity: FeedbackIdentity = {
  installedVersion: "0.1.6",
  platform: "Windows",
};

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    json: async () => body,
  } as unknown as Response;
}

describe("feedback diagnostics", () => {
  it("appends version, platform and the screenshot truth to the body", () => {
    const body = useFeedback.getState().composeBody("The tab froze.", identity, true);
    expect(body.startsWith("The tab froze.")).toBe(true);
    expect(body).toContain("- ZIM: 0.1.6");
    expect(body).toContain("- Platform: Windows");
    expect(body).toContain("paste-attach");
  });

  it("says none when no screenshot rides along", () => {
    const block = diagnosticsBlock(identity, false);
    expect(block).toContain("- Screenshot: none");
  });

  it("answers honestly when the host has not named its version", () => {
    const block = diagnosticsBlock({ installedVersion: null, platform: "Linux" }, false);
    expect(block).toContain("(the host has not answered yet)");
  });
});

describe("the browser route", () => {
  it("prefills title and body into the GitHub issue URL", () => {
    const outcome = useFeedback.getState().composeBrowserUrl("Tab froze", "Steps:\n1. open\n");
    expect(outcome.startsWith(`${NEW_ISSUE_URL}?`)).toBe(true);
    const url = new URL(outcome);
    expect(url.searchParams.get("title")).toBe("Tab froze");
    expect(url.searchParams.get("body")).toContain("Steps:");
  });

  it("truncates an over-long body honestly, in words", () => {
    const checked = composeBrowserUrlChecked(
      NEW_ISSUE_URL,
      "t",
      "x".repeat(6000),
      4000,
    );
    expect(checked.truncated).toBe(true);
    const body = new URL(checked.url).searchParams.get("body") ?? "";
    expect(body).toContain("the tail was cut");
    expect(body.length).toBeLessThan(6000);
  });
});

describe("the token route", () => {
  beforeEach(() => {
    useFeedback.setState({ token: "", login: null, signIn: "unknown", sending: false });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("files the issue and answers with its URL and number", async () => {
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      expect(url).toBe(ISSUES_API_URL);
      const payload = JSON.parse((init?.body ?? "") as string) as { title: string; body: string };
      expect(payload.title).toBe("Crash on start");
      expect(payload.body).toContain("- ZIM:");
      return jsonResponse(201, { html_url: "https://github.com/ZaminMC/ZIM/issues/7", number: 7 });
    });
    vi.stubGlobal("fetch", fetchMock);
    useFeedback.setState({ token: "ghs_token_1" });
    const outcome = await useFeedback.getState().send({
      title: "Crash on start",
      details: "It crashed.",
      identity,
      hasScreenshot: false,
    });
    expect(outcome).toEqual({
      kind: "created",
      url: "https://github.com/ZaminMC/ZIM/issues/7",
      issueNumber: 7,
    });
    // The label ride: a feedback report tags itself.
    const payload = JSON.parse((fetchMock.mock.calls[0]?.[1]?.body ?? "") as string) as { labels?: string[] };
    expect(payload.labels).toEqual(["feedback"]);
    expect(useFeedback.getState().sending).toBe(false);
  });

  it("retries without labels when GitHub answers 422 about the label", async () => {
    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const payload = JSON.parse((init?.body ?? "") as string) as { labels?: string[] };
      if (payload.labels && payload.labels.length > 0) {
        return jsonResponse(422, { message: "label invalid" });
      }
      return jsonResponse(201, { html_url: "https://github.com/ZaminMC/ZIM/issues/8", number: 8 });
    });
    vi.stubGlobal("fetch", fetchMock);
    useFeedback.setState({ token: "ghs_token_1" });
    const outcome = await useFeedback.getState().send({
      title: "t",
      details: "d",
      identity,
      hasScreenshot: false,
    });
    expect(outcome.kind).toBe("created");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("marks the sign-in invalid on a 401 and keeps the report", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(401, { message: "Bad credentials" })));
    useFeedback.setState({ token: "expired" });
    const outcome = await useFeedback.getState().send({
      title: "t",
      details: "d",
      identity,
      hasScreenshot: false,
    });
    expect(outcome.kind).toBe("error");
    if (outcome.kind === "error") expect(outcome.note).toContain("still here");
    expect(useFeedback.getState().signIn).toBe("invalid");
  });

  it("never carries the token in the payload, the URL, or the error", async () => {
    const seen: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init?: RequestInit) => {
        seen.push(url, (init?.body ?? "") as string);
        return jsonResponse(403, { message: "forbidden" });
      }),
    );
    useFeedback.setState({ token: "super_secret_token" });
    const outcome = await useFeedback.getState().send({
      title: "t",
      details: "d",
      identity,
      hasScreenshot: false,
    });
    for (const part of seen) expect(part).not.toContain("super_secret_token");
    expect(outcome.kind).toBe("error");
    if (outcome.kind === "error") expect(outcome.note).not.toContain("super_secret_token");
    // The Authorization header is the only place it may ride.
    const headers = (fetch as ReturnType<typeof vi.fn>).mock.calls[0]?.[1] as RequestInit;
    expect((headers.headers as Record<string, string>).Authorization).toBe(
      "Bearer super_secret_token",
    );
  });
});

describe("sign-in", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("proves the token against /user and names the account", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => jsonResponse(200, { login: "abasing" })),
    );
    useFeedback.setState({ token: "good_token", signIn: "unknown", login: null });
    const result = await useFeedback.getState().checkSignIn();
    expect(result).toBe("signed-in");
    expect(useFeedback.getState().login).toBe("abasing");
  });

  it("answers invalid on 401 instead of pretending", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(401, {})));
    useFeedback.setState({ token: "rotten", signIn: "unknown", login: null });
    const result = await useFeedback.getState().checkSignIn();
    expect(result).toBe("invalid");
  });

  it("stays unknown without a token", async () => {
    useFeedback.setState({ token: "", signIn: "unknown" });
    expect(await useFeedback.getState().checkSignIn()).toBe("unknown");
  });
});
