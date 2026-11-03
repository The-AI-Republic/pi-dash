// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the space engagement writes: vote set/remove and issue
// reaction add/remove, with the method, path and body the board sends.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  addSpaceIssueReaction,
  removeSpaceIssueReaction,
  removeSpaceIssueVote,
  setSpaceIssueVote,
} from "./space-engagement.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";
const DT = "2026-10-20T12:00:00.000Z";

const actor = {
  id: UUID,
  first_name: "T",
  last_name: "Tester",
  avatar: "",
  avatar_url: null,
  is_bot: false,
  display_name: "Tester",
};

function clientFor(value: unknown, status = 200) {
  const transport = recordingTransport((call) => {
    if (call.url.endsWith("/auth/get-csrf-token/")) {
      return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
    }
    if (value === undefined) return fakeResponse({ status: 204, body: "" });
    return fakeResponse({ status, body: JSON.stringify(value) });
  });
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space engagement contracts", () => {
  it("sets a vote and parses the stored row", async () => {
    const { client, transport } = clientFor(
      { issue: UUID, vote: 1, workspace: UUID2, project: UUID3, actor: UUID, actor_detail: actor },
      201
    );
    const parsed = await setSpaceIssueVote(client, "anchor123", UUID, 1);
    expect(parsed.vote).toBe(1);
    expect(parsed.actor_detail.display_name).toBe("Tester");
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("POST");
    expect(call?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID}/votes/`);
    expect(call?.init.body).toBe(JSON.stringify({ vote: 1 }));
  });

  it("sends a down vote as -1", async () => {
    const { client, transport } = clientFor(
      { issue: UUID, vote: -1, workspace: UUID2, project: UUID3, actor: UUID, actor_detail: actor },
      201
    );
    await setSpaceIssueVote(client, "anchor123", UUID, -1);
    expect(transport.calls.at(-1)?.init.body).toBe(JSON.stringify({ vote: -1 }));
  });

  it("removes the visitor's vote", async () => {
    const { client, transport } = clientFor(undefined);
    await removeSpaceIssueVote(client, "anchor123", UUID);
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("DELETE");
    expect(call?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID}/votes/`);
  });

  it("adds an issue reaction and parses the stored row", async () => {
    const { client, transport } = clientFor(
      {
        id: UUID,
        actor_detail: actor,
        created_at: DT,
        updated_at: DT,
        deleted_at: null,
        reaction: "1f600",
        created_by: UUID,
        updated_by: null,
        project: UUID3,
        workspace: UUID2,
        actor: UUID,
        issue: UUID2,
      },
      201
    );
    const parsed = await addSpaceIssueReaction(client, "anchor123", UUID2, "1f600");
    expect(parsed.reaction).toBe("1f600");
    expect(transport.calls.at(-1)?.init.body).toBe(JSON.stringify({ reaction: "1f600" }));
  });

  it("removes an issue reaction by code", async () => {
    const { client, transport } = clientFor(undefined);
    await removeSpaceIssueReaction(client, "anchor123", UUID, "1f600");
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("DELETE");
    expect(call?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID}/reactions/1f600/`);
  });
});
