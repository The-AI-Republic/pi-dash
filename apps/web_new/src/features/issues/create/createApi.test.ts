// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create/edit-dialog network helpers (parity/specs/issues.md §11): the
// title generation timeout, the parent search query and key, the parent
// candidate shaping, the cycle/module membership chains, the edit write
// (issue and draft PATCH plus membership), draft publish, and the
// description helpers (AI task, mentions, upload/link). The dialog
// tests cover the hooks end to end.

import { ApiError } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import {
  applyDescriptionAssets,
  applyEditMembership,
  applyCreateMembership,
  DESCRIBE_MONTHLY_LIMIT,
  DESCRIBE_TASK,
  EDITOR_EMPTY_ANSWER,
  EDITOR_FAILED_BODY,
  EDITOR_QUOTA_EXCEEDED_BODY,
  draftInputFromPatch,
  editorFailureMessage,
  isAssistantRateLimited,
  MENTION_RESULT_LIMIT,
  mentionSearchKey,
  PARENT_SEARCH_DEBOUNCE_MS,
  TitleTimeoutError,
  hitIdentifier,
  parentSearchKey,
  parentSearchQueryOptions,
  publishDraft,
  requestDescription,
  requestEditorAnswer,
  requestMentions,
  requestGeneratedTitle,
  selectParentCandidates,
  writeIssueEdit,
  withTimeout,
  type EditTarget,
} from "./createApi.js";
import type { EditMembership } from "./editForm.js";

function hit(id: string, identifier = "ATL", sequence = 1) {
  return {
    id,
    name: `Row ${id}`,
    start_date: null,
    sequence_id: sequence,
    project__name: "Atlas",
    project__identifier: identifier,
    project_id: "11111111-1111-4111-8111-111111111111",
    workspace__slug: "acme",
    state__name: null,
    state__group: null,
    state__color: null,
  };
}

describe("withTimeout", () => {
  it("resolves with a timely answer", async () => {
    await expect(withTimeout(Promise.resolve("ok"), 1000)).resolves.toBe("ok");
  });

  it("rejects a late answer with the timeout error", async () => {
    await expect(withTimeout(new Promise<string>(() => undefined), 5)).rejects.toBeInstanceOf(TitleTimeoutError);
  });

  it("passes a rejection through unchanged", async () => {
    const failure = new Error("offline");
    await expect(withTimeout(Promise.reject(failure), 1000)).rejects.toBe(failure);
  });
});

describe("requestGeneratedTitle", () => {
  it("trims the assistant's answer", async () => {
    const client = {
      sendJson: vi.fn().mockResolvedValue({ data: { title: "  Named  " } }),
      parse: (_schema: unknown, payload: unknown) => payload,
    };
    await expect(requestGeneratedTitle(client as never, "acme", "<p>Body</p>")).resolves.toBe("Named");
    expect(client.sendJson).toHaveBeenCalledWith("POST", "/api/workspaces/acme/ai-assistant/generate-title/", {
      json: { description: "<p>Body</p>" },
    });
  });
});

describe("parent search", () => {
  it("accepts the half-second debounce from the row", () => {
    expect(PARENT_SEARCH_DEBOUNCE_MS).toBe(500);
  });

  it("keys each project and term separately", () => {
    expect(parentSearchKey("acme", "p", "par")).not.toEqual(parentSearchKey("acme", "p", ""));
    expect(parentSearchKey("acme", "p", "par")).toEqual(parentSearchKey("acme", "p", "par"));
  });

  it("asks the project search with the parent flag and no issue exclusion", async () => {
    const getJson = vi.fn().mockResolvedValue([]);
    const client = { getJson, parse: (_schema: unknown, payload: unknown) => payload };
    const options = parentSearchQueryOptions(client as never, "acme", "p", "par");
    await options.queryFn();
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/projects/p/search-issues/", {
      query: {
        search: "par",
        workspace_search: undefined,
        parent: "true",
        issue_relation: undefined,
        sub_issue: undefined,
        issue_id: undefined,
      },
    });
    expect(options.enabled).toBe(true);
  });

  it("stays disabled without a project", () => {
    const client = { getJson: vi.fn(), parse: (_schema: unknown, payload: unknown) => payload };
    expect(parentSearchQueryOptions(client as never, "acme", "", "").enabled).toBe(false);
  });

  it("labels a hit with its short code and sequence", () => {
    expect(hitIdentifier(hit("a", "NF", 42))).toBe("NF-42");
  });

  it("never offers the current parent again", () => {
    const rows = [hit("a"), hit("b")];
    expect(selectParentCandidates(rows, "a").map((row) => row.id)).toEqual(["b"]);
    expect(selectParentCandidates(rows, null)).toHaveLength(2);
  });
});

describe("applyCreateMembership", () => {
  function stubClient() {
    const posts: Array<{ path: string; json: unknown }> = [];
    const client = {
      sendJson: vi.fn(async (method: string, path: string, options: { json: unknown }) => {
        posts.push({ path, json: options.json });
        return { data: { message: "ok" } };
      }),
      parse: (_schema: unknown, payload: unknown) => payload,
    };
    return { client, posts };
  }

  it("attaches the cycle first, then the modules", async () => {
    const { client, posts } = stubClient();
    await applyCreateMembership(client as never, "acme", {
      projectId: "p",
      issueId: "issue",
      cycleId: "cycle",
      moduleIds: ["m1", "m2"],
    });
    expect(posts).toEqual([
      { path: "/api/workspaces/acme/projects/p/cycles/cycle/cycle-issues/", json: { issues: ["issue"] } },
      { path: "/api/workspaces/acme/projects/p/issues/issue/modules/", json: { modules: ["m1", "m2"] } },
    ]);
  });

  it("makes no calls for an item with neither", async () => {
    const { client, posts } = stubClient();
    await applyCreateMembership(client as never, "acme", {
      projectId: "p",
      issueId: "issue",
      cycleId: null,
      moduleIds: [],
    });
    expect(posts).toEqual([]);
  });

  it("stops when the cycle attach fails, leaving modules for a retry", async () => {
    const client = {
      sendJson: vi.fn(async () => {
        throw new Error("cycle down");
      }),
      parse: (_schema: unknown, payload: unknown) => payload,
    };
    await expect(
      applyCreateMembership(client as never, "acme", {
        projectId: "p",
        issueId: "issue",
        cycleId: "cycle",
        moduleIds: ["m1"],
      })
    ).rejects.toThrow("cycle down");
    expect(client.sendJson).toHaveBeenCalledTimes(1);
  });
});

const PROJECT = "11111111-1111-4111-8111-111111111111";
const ISSUE = "22222222-2222-4222-8222-222222222222";
const CYCLE_A = "33333333-3333-4333-8333-333333333333";
const CYCLE_B = "44444444-4444-4444-8444-444444444444";

const NO_MEMBERSHIP: EditMembership = {
  removedCycleId: null,
  addedCycleId: null,
  addedModuleIds: [],
  removedModuleIds: [],
  cycleId: null,
  moduleIds: [],
};

interface Recorded {
  method: string;
  path: string;
  body?: unknown;
}

/** A client whose every write is recorded; reads answer the given value. */
function recorder(read: unknown = []) {
  const calls: Recorded[] = [];
  const record =
    (method: string) =>
    (...args: unknown[]) => {
      const [methodOrPath, maybePath, options] =
        args[0] === "GET" || args[0] === "POST" || args[0] === "PATCH" || args[0] === "DELETE"
          ? args
          : [undefined, ...args];
      const path = String(maybePath);
      calls.push({
        method: method === "SEND" ? String(methodOrPath) : method,
        path,
        body: (options as { json?: unknown } | undefined)?.json,
      });
      return Promise.resolve(method === "SEND" ? { data: read, status: 200, url: path } : read);
    };
  return {
    calls,
    client: {
      getJson: vi.fn((path: string, options?: { query?: unknown }) => {
        calls.push({ method: "GET", path, body: options?.query });
        return Promise.resolve(read);
      }),
      patch: vi.fn(record("PATCH")),
      delete: vi.fn(record("DELETE")),
      post: vi.fn(record("POST")),
      sendJson: vi.fn(record("SEND")),
      parse: (_schema: unknown, payload: unknown) => payload,
    } as never,
  };
}

function target(): EditTarget {
  return { kind: "issue", projectId: PROJECT, issueId: ISSUE };
}

describe("create membership", () => {
  it("attaches the cycle before the modules", async () => {
    const { calls, client } = recorder();
    await applyCreateMembership(client, "acme", {
      projectId: PROJECT,
      issueId: ISSUE,
      cycleId: CYCLE_A,
      moduleIds: ["m-1", "m-2"],
    });
    expect(calls.map((call) => call.method)).toEqual(["POST", "POST"]);
    expect(calls[0]?.path).toContain(`/projects/${PROJECT}/cycles/${CYCLE_A}/cycle-issues/`);
    expect(calls[0]?.body).toEqual({ issues: [ISSUE] });
    expect(calls[1]?.path).toContain(`/projects/${PROJECT}/issues/${ISSUE}/modules/`);
    expect(calls[1]?.body).toEqual({ modules: ["m-1", "m-2"] });
  });

  it("writes nothing for an item with no cycle or modules", async () => {
    const { calls, client } = recorder();
    await applyCreateMembership(client, "acme", {
      projectId: PROJECT,
      issueId: ISSUE,
      cycleId: null,
      moduleIds: [],
    });
    expect(calls).toEqual([]);
  });
});

describe("applyEditMembership", () => {
  it("never touches bridges for a draft", async () => {
    const { calls, client } = recorder();
    await applyEditMembership(
      client,
      "acme",
      { kind: "draft", draftId: "d-1" },
      {
        removedCycleId: CYCLE_A,
        addedCycleId: CYCLE_B,
        addedModuleIds: ["m-1"],
        removedModuleIds: ["m-2"],
        cycleId: CYCLE_B,
        moduleIds: ["m-1"],
      }
    );
    expect(calls).toEqual([]);
  });

  it("detaches the old cycle, attaches the new one, then diffs modules", async () => {
    const { calls, client } = recorder();
    await applyEditMembership(client, "acme", target(), {
      removedCycleId: CYCLE_A,
      addedCycleId: CYCLE_B,
      addedModuleIds: ["m-1"],
      removedModuleIds: ["m-2"],
      cycleId: CYCLE_B,
      moduleIds: ["m-1"],
    });
    expect(calls).toHaveLength(3);
    expect(calls[0]?.method).toBe("DELETE");
    expect(calls[0]?.path).toContain(`/projects/${PROJECT}/cycles/${CYCLE_A}/cycle-issues/${ISSUE}/`);
    expect(calls[1]?.method).toBe("POST");
    expect(calls[1]?.path).toContain(`/projects/${PROJECT}/cycles/${CYCLE_B}/cycle-issues/`);
    expect(calls[2]?.path).toContain(`/projects/${PROJECT}/issues/${ISSUE}/modules/`);
    expect(calls[2]?.body).toEqual({ modules: ["m-1"], removed_modules: ["m-2"] });
  });

  it("omits removed_modules when no module was removed", async () => {
    const { calls, client } = recorder();
    await applyEditMembership(client, "acme", target(), {
      ...NO_MEMBERSHIP,
      addedModuleIds: ["m-1"],
    });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.body).toEqual({ modules: ["m-1"] });
  });

  it("stops the chain when a bridge write fails", async () => {
    const calls: string[] = [];
    const client = {
      delete: vi.fn(() => {
        calls.push("delete");
        return Promise.reject(new Error("bridge gone"));
      }),
      post: vi.fn((path: string) => {
        calls.push(`post:${path}`);
        return Promise.resolve(undefined);
      }),
    } as never;
    await expect(
      applyEditMembership(client, "acme", target(), {
        removedCycleId: CYCLE_A,
        addedCycleId: CYCLE_B,
        addedModuleIds: [],
        removedModuleIds: [],
        cycleId: CYCLE_B,
        moduleIds: [],
      })
    ).rejects.toThrow("bridge gone");
    expect(calls).toEqual(["delete"]);
  });
});

describe("writeIssueEdit", () => {
  it("PATCHes a live issue with its fields, then its bridges", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: target(),
      patch: { name: "Renamed" },
      membership: { ...NO_MEMBERSHIP, addedCycleId: CYCLE_B },
    });
    expect(calls.map((call) => call.method)).toEqual(["PATCH", "POST"]);
    expect(calls[0]?.path).toContain(`/projects/${PROJECT}/issues/${ISSUE}/`);
    expect(calls[0]?.body).toEqual({ name: "Renamed" });
  });

  it("skips the PATCH when nothing changed but still reconciles bridges", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: target(),
      patch: {},
      membership: { ...NO_MEMBERSHIP, addedCycleId: CYCLE_B },
    });
    expect(calls.map((call) => call.method)).toEqual(["POST"]);
  });

  it("stores a picked project on a project-less draft with no field changes (ISS-126/130)", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: { kind: "draft", draftId: "d-1" },
      patch: {},
      membership: NO_MEMBERSHIP,
      draftProject: PROJECT,
    });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.path).toContain("/draft-issues/d-1/");
    expect(calls[0]?.body).toEqual({ project_id: PROJECT });
  });

  it("PATCHes a draft's fields, cycle, modules and estimate, and never its bridges", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: { kind: "draft", draftId: "d-1" },
      patch: { name: "Draft renamed", estimate_point: "e-2", git_work_branch: "team/x" },
      membership: {
        ...NO_MEMBERSHIP,
        removedCycleId: CYCLE_A,
        addedCycleId: CYCLE_B,
        addedModuleIds: ["m-1"],
        removedModuleIds: ["m-2"],
        cycleId: CYCLE_B,
        moduleIds: ["m-1", "m-3"],
      },
    });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.path).toContain("/draft-issues/d-1/");
    expect(calls[0]?.body).toEqual({
      name: "Draft renamed",
      estimate_point: "e-2",
      cycle_id: CYCLE_B,
      module_ids: ["m-1", "m-3"],
    });
  });

  it("clears a draft's cycle without touching its modules", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: { kind: "draft", draftId: "d-1" },
      patch: {},
      membership: { ...NO_MEMBERSHIP, removedCycleId: CYCLE_A, cycleId: null },
    });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.body).toEqual({ cycle_id: null });
  });

  it("sends no draft write when only unmapped fields changed", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: { kind: "draft", draftId: "d-1" },
      patch: { git_work_branch: "" },
      membership: NO_MEMBERSHIP,
    });
    expect(calls).toEqual([]);
  });

  it("skips a draft write with no fields", async () => {
    const { calls, client } = recorder();
    await writeIssueEdit(client, "acme", {
      target: { kind: "draft", draftId: "d-1" },
      patch: {},
      membership: NO_MEMBERSHIP,
    });
    expect(calls).toEqual([]);
  });
});

describe("draftInputFromPatch", () => {
  it("keeps only the fields a draft stores", () => {
    expect(
      draftInputFromPatch({
        name: "n",
        description_html: "<p>b</p>",
        state_id: "s",
        priority: "high",
        parent_id: "p",
        assignee_ids: ["u"],
        label_ids: ["l"],
        start_date: "2026-01-01",
        target_date: null,
        git_work_branch: "team/x",
        assigned_pod_id: "pod",
        assigned_runner_id: "runner-1",
        agent_executor: "local_runner",
        estimate_point: "e-1",
      })
    ).toEqual({
      name: "n",
      description_html: "<p>b</p>",
      state_id: "s",
      priority: "high",
      parent_id: "p",
      assignee_ids: ["u"],
      label_ids: ["l"],
      start_date: "2026-01-01",
      target_date: null,
      estimate_point: "e-1",
    });
  });

  it("maps an empty patch to an empty draft input", () => {
    expect(draftInputFromPatch({})).toEqual({});
  });

  it("carries a picked project for a project-less draft (ISS-130)", () => {
    expect(draftInputFromPatch({}, undefined, PROJECT)).toEqual({ project_id: PROJECT });
    expect(draftInputFromPatch({}, undefined, null)).toEqual({ project_id: null });
  });
});

describe("publishDraft", () => {
  it("posts the draft into its project through the promote endpoint", async () => {
    const promoted = { id: ISSUE, project_id: PROJECT };
    const { calls, client } = recorder(promoted);
    await expect(publishDraft(client, "acme", "d-1", { name: "Publish", project_id: PROJECT }, PROJECT)).resolves.toBe(
      promoted
    );
    expect(calls[0]?.method).toBe("POST");
    expect(calls[0]?.path).toContain("/draft-to-issue/d-1/");
    expect(calls).toHaveLength(1);
  });

  it("stores a newly picked project on the draft before promoting it", async () => {
    // The promote reads the draft's stored project, so a draft that only
    // just picked one must be patched first (ISS-130).
    const promoted = { id: ISSUE, project_id: PROJECT };
    const { calls, client } = recorder(promoted);
    await expect(publishDraft(client, "acme", "d-1", { name: "Publish", project_id: PROJECT }, null)).resolves.toBe(
      promoted
    );
    expect(calls[0]?.method).toBe("PATCH");
    expect(calls[0]?.path).toContain("/draft-issues/d-1/");
    expect(calls[0]?.body).toEqual({ project_id: PROJECT });
    expect(calls[1]?.method).toBe("POST");
    expect(calls[1]?.path).toContain("/draft-to-issue/d-1/");
  });

  it("leaves a draft whose stored project already matches alone", async () => {
    const { calls, client } = recorder({ id: ISSUE, project_id: PROJECT });
    // A client that records nothing would hide a stray write: PATCH is
    // recorded even when the body is thin, so count the calls.
    await publishDraft(client, "acme", "d-1", { name: "Publish", project_id: PROJECT }, PROJECT);
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(calls[0]?.path).toContain("/draft-to-issue/d-1/");
  });
});

describe("description assistant", () => {
  it("runs the describe task with the title as its prompt", async () => {
    const { calls, client } = recorder({ response_html: "  <p>made</p>  " });
    await expect(requestDescription(client, "acme", PROJECT, "Fix the thing")).resolves.toBe("<p>made</p>");
    expect(calls[0]?.method).toBe("POST");
    expect(calls[0]?.path).toContain(`/projects/${PROJECT}/`);
    expect(calls[0]?.body).toEqual({ task: DESCRIBE_TASK, prompt: "Fix the thing" });
  });

  it("runs a free-form prompt with an empty prompt on the wire", async () => {
    const { calls, client } = recorder({ response: "answer", response_html: "  <p>answer</p>  " });
    await expect(requestEditorAnswer(client, "acme", PROJECT, "Make it shorter")).resolves.toEqual({
      text: "answer",
      html: "<p>answer</p>",
    });
    expect(calls[0]?.body).toEqual({ task: "Make it shorter", prompt: "" });
  });

  it("publishes the monthly limit constant used in the toast", () => {
    expect(DESCRIBE_MONTHLY_LIMIT).toBe(50);
  });

  it("reads the quota line for a bare 429 and the server message when one rides along", () => {
    expect(editorFailureMessage(new ApiError({ status: 429, code: "http", message: "{}", body: {} }))).toBe(
      EDITOR_QUOTA_EXCEEDED_BODY
    );
    expect(
      editorFailureMessage(new ApiError({ status: 429, code: "http", message: "slow down", body: { error: "slow" } }))
    ).toBe("slow");
    expect(
      editorFailureMessage(
        new ApiError({ status: 429, code: "http", message: "slow down", body: { detail: "slow down" } })
      )
    ).toBe("slow down");
    expect(
      editorFailureMessage(
        new ApiError({
          status: 429,
          code: "http",
          message: "both",
          body: { detail: "detail wins", error: "code loses" },
        })
      )
    ).toBe("detail wins");
    expect(editorFailureMessage(new ApiError({ status: 429, code: "http", message: "", body: { error: "" } }))).toBe(
      EDITOR_QUOTA_EXCEEDED_BODY
    );
    expect(editorFailureMessage(new ApiError({ status: 429, code: "http", message: "", body: { detail: "" } }))).toBe(
      EDITOR_QUOTA_EXCEEDED_BODY
    );
  });

  it("reads any other failure through the server's own line", () => {
    expect(
      editorFailureMessage(
        new ApiError({
          status: 400,
          code: "http",
          message: "LLM provider API key and model are required",
          body: { error: "LLM provider API key and model are required" },
        })
      )
    ).toBe("LLM provider API key and model are required");
    expect(
      editorFailureMessage(
        new ApiError({ status: 500, code: "http", message: "server says", body: { detail: "server says" } })
      )
    ).toBe("server says");
  });

  it("falls back when a failure carries no server line", () => {
    expect(editorFailureMessage(new ApiError({ status: 500, code: "http", message: "", body: null }))).toBe(
      EDITOR_FAILED_BODY
    );
    expect(editorFailureMessage(new ApiError({ status: 500, code: "http", message: "{}", body: {} }))).toBe(
      EDITOR_FAILED_BODY
    );
    expect(editorFailureMessage(new ApiError({ status: 500, code: "http", message: "lifted", body: "oops" }))).toBe(
      EDITOR_FAILED_BODY
    );
    expect(editorFailureMessage(new Error("network down"))).toBe(EDITOR_FAILED_BODY);
    expect(editorFailureMessage(null)).toBe(EDITOR_FAILED_BODY);
    expect(editorFailureMessage("boom")).toBe(EDITOR_FAILED_BODY);
    expect(EDITOR_EMPTY_ANSWER).not.toBe("");
  });

  it("reads quota only off a 429 status", () => {
    expect(isAssistantRateLimited(new ApiError({ status: 429, code: "http", message: "x", body: {} }))).toBe(true);
    expect(isAssistantRateLimited(new ApiError({ status: 500, code: "http", message: "x", body: {} }))).toBe(false);
    expect(isAssistantRateLimited(null)).toBe(false);
    expect(isAssistantRateLimited("nope")).toBe(false);
  });
});

describe("mentions", () => {
  it("asks for people in the project and shapes them for the popup", async () => {
    const { calls, client } = recorder({
      user_mention: [{ member__id: "u-1", member__display_name: "Ada", member__avatar_url: null }],
    });
    await expect(requestMentions(client, "acme", PROJECT, "ad")).resolves.toEqual({
      people: [{ id: "u-1", name: "Ada" }],
    });
    expect(calls[0]?.path).toContain("/api/workspaces/acme/entity-search/");
    expect(calls[0]?.body).toMatchObject({
      query: "ad",
      query_type: "user_mention",
      project_id: PROJECT,
      count: MENTION_RESULT_LIMIT,
    });
  });

  it("returns nobody without a project", async () => {
    const { calls, client } = recorder();
    await expect(requestMentions(client, "acme", "", "ad")).resolves.toEqual({ people: [] });
    expect(calls).toEqual([]);
  });

  it("keys each project and term separately", () => {
    expect(mentionSearchKey("acme", PROJECT, "ad")).not.toEqual(mentionSearchKey("acme", PROJECT, "gr"));
    expect(mentionSearchKey("acme", PROJECT, "ad")).not.toEqual(mentionSearchKey("acme", "other", "ad"));
  });
});

describe("description assets", () => {
  it("links parked assets to the row", async () => {
    const { calls, client } = recorder();
    await applyDescriptionAssets(client, "acme", PROJECT, ISSUE, ["a-1"]);
    expect(calls[0]?.method).toBe("POST");
    expect(calls[0]?.path).toContain(`/projects/${PROJECT}/${ISSUE}/bulk/`);
    expect(calls[0]?.body).toEqual({ asset_ids: ["a-1"] });
  });
});
