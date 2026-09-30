// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { createClient, type Transport } from "@pidash/api-client";
import { issueKeys } from "./keys.js";
import { issuesQueryOptions } from "./queries.js";

function stubTransport(body: unknown): Transport {
  return async () => ({
    status: 200,
    url: "https://api.test/",
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
}

const ENVELOPE = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 0,
  next_cursor: "",
  prev_cursor: "",
  next_page_results: false,
  prev_page_results: false,
  count: 0,
  total_results: 0,
  results: [],
};

describe("issue keys", () => {
  it("scopes every key under the workspace", () => {
    expect(issueKeys.list("acme", "pid", { layout: "list" })[0]).toBe("ws");
    expect(issueKeys.list("acme", "pid", { layout: "list" })[1]).toBe("acme");
    expect(issueKeys.detail("acme", "iid")).toEqual(["ws", "acme", "issues", "detail", "iid"]);
  });

  it("separates projects and view states", () => {
    expect(issueKeys.list("acme", "a", { layout: "list" })).not.toEqual(
      issueKeys.list("acme", "b", { layout: "list" })
    );
    expect(issueKeys.list("acme", "a", { layout: "list" })).not.toEqual(
      issueKeys.list("acme", "a", { layout: "compact" })
    );
  });
});

describe("issues query", () => {
  it("reads the list through the workspace-scoped URL", async () => {
    const seen: string[] = [];
    const inner = stubTransport(ENVELOPE);
    const client = createClient({
      baseUrl: "https://api.test",
      transport: async (url, init) => {
        seen.push(url);
        return inner(url, init);
      },
      validate: true,
    });
    const options = issuesQueryOptions(client, "acme", "pid", { layout: "list" });
    expect(options.queryKey[0]).toBe("ws");
    const data = await new QueryClient().fetchQuery(options);
    expect(data.total_count).toBe(0);
    expect(seen[0]).toBe("https://api.test/api/workspaces/acme/projects/pid/issues/?per_page=50&order_by=-created_at");
  });
});
