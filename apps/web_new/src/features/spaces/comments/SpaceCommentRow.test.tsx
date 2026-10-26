// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceComment } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SpaceCommentRow } from "./SpaceCommentRow.js";

const ADA = "523e4567-e89b-12d3-a456-426614174004";

function comment(overrides: Partial<SpaceComment> = {}): SpaceComment {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    actor: ADA,
    actor_detail: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    issue: "223e4567-e89b-12d3-a456-426614174001",
    project: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "423e4567-e89b-12d3-a456-426614174003",
    comment_html: "<p>Hello <strong>there</strong></p>",
    comment_stripped: "Hello there",
    comment_json: {},
    attachments: [],
    labels: [],
    access: "EXTERNAL",
    edited_at: null,
    comment_reactions: [],
    speaker_type: "human",
    speaker_label: "",
    speaker_agent_run_id: null,
    parent: null,
    external_source: null,
    external_id: null,
    is_synced: false,
    description: null,
    created_at: new Date(Date.now() - 2 * 60 * 60_000).toISOString(),
    updated_at: "2026-10-16T22:00:00Z",
    created_by: ADA,
    updated_by: null,
    deleted_at: null,
    ...overrides,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("SpaceCommentRow", () => {
  it("renders identity, relative age and formatted content (SPC-050)", () => {
    render(<SpaceCommentRow comment={comment()} resolveAsset={(src) => src} />);
    expect(screen.getByText("Ada L")).toBeInTheDocument();
    expect(screen.getByText(/commented 2 hours ago/)).toBeInTheDocument();
    expect(screen.getByText("there", { selector: "strong" })).toBeInTheDocument();
    expect(screen.queryByText(/Bot/)).not.toBeInTheDocument();
  });

  it("marks a bot in its identity and falls back to an initial avatar", () => {
    render(
      <SpaceCommentRow
        comment={comment({ actor_detail: { ...comment().actor_detail!, is_bot: true } })}
        resolveAsset={(src) => src}
      />
    );
    expect(screen.getByText("Ada Bot")).toBeInTheDocument();
    expect(screen.getByText("A")).toBeInTheDocument();
  });

  it("renders the author's stored avatar image when available", () => {
    render(
      <SpaceCommentRow
        comment={comment({ actor_detail: { ...comment().actor_detail!, avatar_url: "avatar-1.png" } })}
        resolveAsset={(src) => `/assets/${src}`}
      />
    );
    expect(screen.getByRole("img", { name: "Ada L" }).querySelector("img")).toHaveAttribute("src", "avatar-1.png");
  });

  it("renders an own-line speaker label only for agent rows (SPC-052)", () => {
    const { rerender } = render(
      <SpaceCommentRow
        comment={comment({ speaker_type: "agent", speaker_label: "Probe Agent" })}
        resolveAsset={(src) => src}
      />
    );
    expect(screen.getByText("Probe Agent:")).toBeInTheDocument();

    rerender(
      <SpaceCommentRow comment={comment({ speaker_type: "agent", speaker_label: "" })} resolveAsset={(src) => src} />
    );
    expect(screen.getByText("Agent:")).toBeInTheDocument();
  });

  it("keeps folded bodies collapsed and toggles them in place (SPC-051)", async () => {
    const user = userEvent.setup();
    render(<SpaceCommentRow comment={comment({ labels: ["fold"] })} resolveAsset={(src) => src} />);
    const toggle = screen.getByRole("button", { name: "Show moderated comment" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("there", { selector: "strong" })).not.toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(toggle).toHaveTextContent("Hide moderated comment");
    expect(screen.getByText("there", { selector: "strong" })).toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveTextContent("Show moderated comment");
    expect(screen.queryByText("there", { selector: "strong" })).not.toBeInTheDocument();
  });

  it("does not offer a fold toggle on an ordinary row", () => {
    render(<SpaceCommentRow comment={comment()} resolveAsset={(src) => src} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("escapes stripped text when the stored HTML is empty", () => {
    render(
      <SpaceCommentRow comment={comment({ comment_html: "", comment_stripped: "A & B" })} resolveAsset={(src) => src} />
    );
    expect(screen.getByText("A & B")).toBeInTheDocument();
  });
});
