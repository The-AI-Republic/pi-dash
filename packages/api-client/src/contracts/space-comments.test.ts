// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the space comment thread: list, create, edit, delete and
// the flat-shaped comment reactions.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  addSpaceCommentReaction,
  createSpaceComment,
  deleteSpaceComment,
  listSpaceComments,
  removeSpaceCommentReaction,
  updateSpaceComment,
} from "./space-comments.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const UUID3 = "323e4567-e89b-12d3-a456-426614174002";
const DT = "2026-10-20T12:00:00.000Z";

const reaction = {
  id: UUID,
  actor: UUID2,
  comment: UUID3,
  reaction: "1f600",
  display_name: "Tester",
  deleted_at: null,
  workspace: UUID2,
  project: UUID3,
  created_at: DT,
  updated_at: DT,
  created_by: UUID2,
  updated_by: null,
};

const comment = {
  id: UUID,
  actor: UUID2,
  actor_detail: {
    id: UUID2,
    first_name: "T",
    last_name: "Tester",
    avatar: "",
    avatar_url: null,
    is_bot: false,
    display_name: "Tester",
  },
  issue: UUID3,
  project: UUID3,
  workspace: UUID2,
  comment_html: "<p>Hello</p>",
  comment_stripped: "Hello",
  comment_json: {},
  attachments: [],
  labels: [],
  access: "EXTERNAL",
  edited_at: null,
  comment_reactions: [reaction],
  speaker_type: "human",
  speaker_label: "",
  speaker_agent_run_id: null,
  parent: null,
  external_source: null,
  external_id: null,
  is_synced: false,
  description: null,
  created_at: DT,
  updated_at: DT,
  created_by: UUID2,
  updated_by: UUID2,
  deleted_at: null,
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

describe("space comment contracts", () => {
  it("lists comments with the flat reaction shape", async () => {
    const { client, transport } = clientFor([comment]);
    const parsed = await listSpaceComments(client, "anchor123", UUID3);
    expect(parsed[0]?.comment_html).toBe("<p>Hello</p>");
    expect(parsed[0]?.comment_reactions[0]?.display_name).toBe("Tester");
    expect(transport.calls.at(-1)?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID3}/comments/`);
  });

  it("creates a comment with EXTERNAL access and the body nodes", async () => {
    const { client, transport } = clientFor(comment, 201);
    await createSpaceComment(client, "anchor123", UUID3, {
      commentHtml: "<p>Hello</p>",
      commentJson: { type: "doc" },
    });
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("POST");
    expect(JSON.parse(call?.init.body ?? "{}")).toEqual({
      comment_html: "<p>Hello</p>",
      comment_json: { type: "doc" },
      access: "EXTERNAL",
    });
  });

  it("omits comment_json when the caller does not send one", async () => {
    const { client, transport } = clientFor(comment, 201);
    await createSpaceComment(client, "anchor123", UUID3, { commentHtml: "<p>Hi</p>" });
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      comment_html: "<p>Hi</p>",
      access: "EXTERNAL",
    });
  });

  it("edits a comment with only the fields supplied", async () => {
    const { client, transport } = clientFor(comment);
    await updateSpaceComment(client, "anchor123", UUID3, UUID, { commentHtml: "<p>Edited</p>" });
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("PATCH");
    expect(JSON.parse(call?.init.body ?? "{}")).toEqual({ comment_html: "<p>Edited</p>" });
  });

  it("sends the structured body on an edit when supplied", async () => {
    const { client, transport } = clientFor(comment);
    await updateSpaceComment(client, "anchor123", UUID3, UUID, {
      commentJson: { type: "doc", content: [] },
    });
    expect(JSON.parse(transport.calls.at(-1)?.init.body ?? "{}")).toEqual({
      comment_json: { type: "doc", content: [] },
    });
  });

  it("deletes the visitor's own comment", async () => {
    const { client, transport } = clientFor(undefined);
    await deleteSpaceComment(client, "anchor123", UUID3, UUID);
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("DELETE");
    expect(call?.url).toBe(`https://api.test/api/public/anchor/anchor123/issues/${UUID3}/comments/${UUID}/`);
  });

  it("adds a comment reaction and parses the flat row", async () => {
    const { client, transport } = clientFor(reaction, 201);
    const parsed = await addSpaceCommentReaction(client, "anchor123", UUID3, "1f600");
    expect(parsed.display_name).toBe("Tester");
    expect(transport.calls.at(-1)?.url).toBe(
      `https://api.test/api/public/anchor/anchor123/comments/${UUID3}/reactions/`
    );
    expect(transport.calls.at(-1)?.init.body).toBe(JSON.stringify({ reaction: "1f600" }));
  });

  it("rejects an issue-shaped reaction with a nested actor", async () => {
    const { client } = clientFor({ ...reaction, display_name: undefined }, 201);
    await expect(addSpaceCommentReaction(client, "anchor123", UUID3, "1f600")).rejects.toThrow();
  });

  it("removes a comment reaction by code", async () => {
    const { client, transport } = clientFor(undefined);
    await removeSpaceCommentReaction(client, "anchor123", UUID3, "1f600");
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("DELETE");
    expect(call?.url).toBe(`https://api.test/api/public/anchor/anchor123/comments/${UUID3}/reactions/1f600/`);
  });
});
