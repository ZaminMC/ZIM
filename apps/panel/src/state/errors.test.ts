// The error translation's copy law: protocol method names, IPC wording,
// and daemon internals never surface as a title. The technical detail
// rides the remediation lines and the context, where a developer view
// can find it and normal UI can ignore it.

import { describe, expect, it } from "vitest";
import {
  ConnectionLostError,
  DisposedError,
  RequestTimeoutError,
} from "../protocol/client";
import { describeError } from "./errors";

describe("describeError", () => {
  it("names the human request, never the method, when a timeout lands", () => {
    const described = describeError(new RequestTimeoutError("catalog.list", 10_000));
    expect(described.title).toBe("Unable to load the server catalog.");
    expect(described.title).not.toContain("catalog.list");
    // The technical line lives in the remediation, not the headline.
    expect(described.remediation.some((line) => line.includes("catalog.list"))).toBe(true);
  });

  it("maps the other heavy families to their own titles", () => {
    expect(describeError(new RequestTimeoutError("server.discover", 60_000)).title).toBe(
      "The scan could not finish.",
    );
    expect(describeError(new RequestTimeoutError("backup.create", 180_000)).title).toBe(
      "The backup operation did not finish.",
    );
    expect(describeError(new RequestTimeoutError("plugins.search", 20_000)).title).toBe(
      "Unable to reach the plugin catalog.",
    );
    expect(describeError(new RequestTimeoutError("daemon.whatever", 10_000)).title).toBe(
      "The request took too long.",
    );
  });

  it("speaks the reconnect promise when the wire drops", () => {
    const described = describeError(new ConnectionLostError());
    expect(described.title).toBe("The connection to ZIM dropped.");
    expect(described.remediation.join(" ")).toContain("reconnects");
  });

  it("treats a disposed client as a shutdown, not an error story", () => {
    expect(describeError(new DisposedError()).title).toBe("ZIM is shutting down.");
  });
});
