// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the NEWFRONT-265 seed-settling block appended to ./api.ts:
// the seed-project match predicate, the Django cleanup-snippet builder, the
// drain-marker parser, and the cookie-session seed wait (global fetch
// stubbed, so no stack). The AuthedSession variants delegate to the same
// tested cores with a different lister, and the apiShell-backed drain loop
// is exercised by the oracle specs that call it.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  awaitWorkspaceSeededProjectWithSession,
  isWorkspaceSeedProject,
  seedDrainOf,
  workspaceSeedCleanupPython,
} from "./api.js";

interface SeenFetch {
  url: unknown;
  init: unknown;
}

function jsonResponse(payload: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(payload),
  } as unknown as Response;
}

function stubFetchResponder(respond: () => Response): { seen: SeenFetch[] } {
  const seen: SeenFetch[] = [];
  const fake = ((url: unknown, init: unknown) => {
    seen.push({ url, init });
    return Promise.resolve(respond());
  }) as unknown as typeof fetch;
  vi.stubGlobal("fetch", fake);
  return { seen };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("isWorkspaceSeedProject", () => {
  it("matches the project named after the workspace", () => {
    expect(isWorkspaceSeedProject("Empty WS abc123", "Empty WS abc123")).toBe(true);
  });

  it("does not match the seed file's demo title", () => {
    // The seed file calls its project "Pi Dash Demo Project", but the seed
    // task discards that title and names the created project after the
    // workspace instead: matching the demo title would never drain.
    expect(isWorkspaceSeedProject("Pi Dash Demo Project", "Empty WS abc123")).toBe(false);
  });

  it("does not match scenario projects", () => {
    expect(isWorkspaceSeedProject("Real Project", "Empty WS abc123")).toBe(false);
  });

  it("is exact: case and whitespace variants do not match", () => {
    expect(isWorkspaceSeedProject("empty ws abc123", "Empty WS abc123")).toBe(false);
    expect(isWorkspaceSeedProject("Empty WS abc123 ", "Empty WS abc123")).toBe(false);
  });
});

describe("workspaceSeedCleanupPython", () => {
  const snippet = workspaceSeedCleanupPython("empty-abc123", "11111111-2222-3333-4444-555555555555");

  it("soft-deletes the seed project by id scoped to the workspace", () => {
    expect(snippet).toContain(
      'proj = Project.objects.filter(id="11111111-2222-3333-4444-555555555555", workspace=ws).first()'
    );
    expect(snippet).toContain("proj.delete()");
  });

  it("removes the bot's workspace-level pages, membership and user", () => {
    expect(snippet).toContain("Page.objects.filter(workspace=ws)");
    expect(snippet).toContain("members = WorkspaceMember.objects.filter(workspace=ws, member=bot).delete()");
    // The bot user has no soft-delete: a plain queryset delete returning
    // the (total, per-model) tuple, which the snippet unpacks.
    expect(snippet).toContain("users, _ = User.objects.filter(id=bot.id).delete()");
  });

  it("prints the drain marker with numeric counts", () => {
    expect(snippet).toContain("PARITY_SEED_DRAIN:");
  });

  it("escapes its inputs", () => {
    const nasty = workspaceSeedCleanupPython('a"b\\c', "id");
    expect(nasty).toContain('Workspace.objects.get(slug="a\\"b\\\\c")');
    expect(nasty).not.toContain('slug="a"b');
  });
});

describe("seedDrainOf", () => {
  it("parses the marker counts", () => {
    expect(seedDrainOf('PARITY_SEED_DRAIN:{"projects": 1, "pages": 12, "members": 1, "users": 1}')).toEqual({
      projects: 1,
      pages: 12,
      members: 1,
      users: 1,
    });
  });

  it("finds the marker amid other Django shell output", () => {
    const out = [
      "3 objects deleted (cascade notes omitted)",
      'PARITY_SEED_DRAIN:{"projects": 0, "pages": 0, "members": 0, "users": 0}',
      "",
    ].join("\n");
    expect(seedDrainOf(out)).toEqual({ projects: 0, pages: 0, members: 0, users: 0 });
  });

  it("throws when the marker row is missing", () => {
    expect(() => seedDrainOf("deleted 3 rows\nno marker here\n")).toThrow("no marker row");
  });

  it("throws when a count is not numeric", () => {
    expect(() => seedDrainOf('PARITY_SEED_DRAIN:{"projects": 1, "pages": "12", "members": 1, "users": 1}')).toThrow(
      "no numeric counts"
    );
  });
});

describe("awaitWorkspaceSeededProjectWithSession", () => {
  const slug = "empty-abc123";
  const workspaceName = "Empty WS abc123";
  const apiBase = "http://parity-api.test";

  it("returns the seed project id and reads through the session cookie", async () => {
    const { seen } = stubFetchResponder(() => jsonResponse([{ id: "seed-id-1", name: workspaceName }]));
    const id = await awaitWorkspaceSeededProjectWithSession("sessionid=abc", slug, workspaceName, apiBase);
    expect(id).toBe("seed-id-1");
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe(`${apiBase}/api/workspaces/${slug}/projects/details/`);
    const init = seen[0]?.init as { headers?: { cookie?: unknown } } | undefined;
    expect(init?.headers?.cookie).toBe("sessionid=abc");
  });

  it("ignores scenario projects and matches the workspace-named seed", async () => {
    stubFetchResponder(() =>
      jsonResponse([
        { id: "real-1", name: "Real Project" },
        { id: "seed-id-2", name: workspaceName },
      ])
    );
    const id = await awaitWorkspaceSeededProjectWithSession("c", slug, workspaceName, apiBase);
    expect(id).toBe("seed-id-2");
  });

  it("polls until the seed appears", async () => {
    const payloads: unknown[] = [[], [{ id: "seed-id-3", name: workspaceName }]];
    let calls = 0;
    stubFetchResponder(() => {
      const next = payloads[Math.min(calls, payloads.length - 1)];
      calls += 1;
      return jsonResponse(next);
    });
    const id = await awaitWorkspaceSeededProjectWithSession("c", slug, workspaceName, apiBase, { pollMs: 1 });
    expect(id).toBe("seed-id-3");
    expect(calls).toBe(2);
  });

  it("throws a worker hint when the seed never appears", async () => {
    const { seen } = stubFetchResponder(() => jsonResponse([]));
    await expect(
      awaitWorkspaceSeededProjectWithSession("c", slug, workspaceName, apiBase, { timeoutMs: 0 })
    ).rejects.toThrow("never appeared");
    expect(seen.length).toBeGreaterThanOrEqual(1);
  });

  it("throws when the project list read fails", async () => {
    stubFetchResponder(() => jsonResponse({ detail: "boom" }, 500));
    await expect(awaitWorkspaceSeededProjectWithSession("c", slug, workspaceName, apiBase)).rejects.toThrow("HTTP 500");
  });

  it("throws when a project row has no string id and name", async () => {
    stubFetchResponder(() => jsonResponse([{ id: 7, name: workspaceName }]));
    await expect(awaitWorkspaceSeededProjectWithSession("c", slug, workspaceName, apiBase)).rejects.toThrow(
      "no string id and name"
    );
  });
});
