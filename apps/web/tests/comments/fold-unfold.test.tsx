/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

const { updateComment, removeComment, copyCommentLink, reactionIds, userReactions, ACCESS } = vi.hoisted(() => ({
  updateComment: vi.fn(),
  removeComment: vi.fn(),
  copyCommentLink: vi.fn(),
  reactionIds: vi.fn(() => ({})),
  userReactions: vi.fn(() => ({})),
  ACCESS: { INTERNAL: "INTERNAL", EXTERNAL: "EXTERNAL" } as const,
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/store/user", () => ({
  useUser: () => ({ data: { id: "user-1" } }),
}));

vi.mock("@pi-dash/constants", () => ({ EIssueCommentAccessSpecifier: ACCESS }));
vi.mock("@pi-dash/types", () => ({ EIssueCommentAccessSpecifier: ACCESS }));

vi.mock("@pi-dash/utils", () => ({
  cn: (...args: unknown[]) => args.filter((a) => typeof a === "string").join(" "),
  getFileURL: (x: unknown) => x,
  calculateTimeAgo: () => "just now",
  renderFormattedDate: () => "2026-09-04",
  renderFormattedTime: () => "12:00",
}));

vi.mock("@pi-dash/propel/icon-button", () => ({
  IconButton: ({ icon: _icon, ...props }: { icon?: unknown } & React.ButtonHTMLAttributes<HTMLButtonElement>) => (
    <button aria-label="comment actions" {...props} />
  ),
}));

vi.mock("@pi-dash/propel/icons", () => {
  // eslint-disable-next-line unicorn/consistent-function-scoping -- inside vi.mock factory; cannot hoist out
  const Icon = (props: React.SVGProps<SVGSVGElement>) => <svg {...props} />;
  return { LinkIcon: Icon, GlobeIcon: Icon, LockIcon: Icon, EditIcon: Icon, TrashIcon: Icon };
});

vi.mock("@pi-dash/ui", async () => {
  const { useState } = await import("react");
  // eslint-disable-next-line unicorn/consistent-function-scoping -- inside vi.mock factory
  const CustomMenu = ({ customButton, children }: { customButton: React.ReactNode; children: React.ReactNode }) => {
    const [open, setOpen] = useState(false);
    return (
      <div>
        {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- test-only menu trigger stub */}
        <span onClick={() => setOpen((o) => !o)}>{customButton}</span>
        {open ? <div role="menu">{children}</div> : null}
      </div>
    );
  };
  CustomMenu.MenuItem = ({
    children,
    onClick,
    disabled,
  }: {
    children: React.ReactNode;
    onClick?: () => void;
    disabled?: boolean;
  }) => (
    <button role="menuitem" onClick={onClick} disabled={disabled}>
      {children}
    </button>
  );
  return { CustomMenu };
});

import { CommentQuickActions } from "@/components/comments/quick-actions";

type AnyComment = Record<string, unknown>;

const baseComment = (overrides: AnyComment = {}): AnyComment => ({
  id: "comment-1",
  actor: "user-1",
  actor_detail: { display_name: "Alice", is_bot: false },
  created_at: "2026-09-04T12:00:00Z",
  comment_html: "<p>Hello body</p>",
  access: ACCESS.EXTERNAL,
  labels: [],
  speaker_type: "human",
  ...overrides,
});

const ops = {
  updateComment,
  removeComment,
  copyCommentLink,
  reactionIds,
  userReactions,
  react: vi.fn(),
};

const renderActions = (comment: AnyComment, showCopyLinkOption = false) =>
  render(
    <CommentQuickActions
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      activityOperations={ops as any}
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      comment={comment as any}
      setEditMode={() => {}}
      showAccessSpecifier={false}
      showCopyLinkOption={showCopyLinkOption}
    />
  );

afterEach(() => vi.clearAllMocks());

describe("CommentQuickActions — fold/unfold action", () => {
  it("shows 'Fold comment' for an unfolded comment and persists the fold label on click", async () => {
    const user = userEvent.setup();
    renderActions(baseComment({ labels: [] }));

    await user.click(screen.getByRole("button", { name: "comment actions" }));
    const menu = screen.getByRole("menu");
    expect(within(menu).getByText("Fold comment")).toBeInTheDocument();
    expect(within(menu).queryByText("Unfold comment")).not.toBeInTheDocument();

    await user.click(within(menu).getByText("Fold comment"));
    expect(updateComment).toHaveBeenCalledWith("comment-1", { labels: ["fold"] });
  });

  it("shows 'Unfold comment' for a folded comment and removes only the fold label on click", async () => {
    const user = userEvent.setup();
    renderActions(baseComment({ labels: ["important", "fold"] }));

    await user.click(screen.getByRole("button", { name: "comment actions" }));
    const menu = screen.getByRole("menu");
    expect(within(menu).getByText("Unfold comment")).toBeInTheDocument();
    expect(within(menu).queryByText("Fold comment")).not.toBeInTheDocument();

    await user.click(within(menu).getByText("Unfold comment"));
    expect(updateComment).toHaveBeenCalledWith("comment-1", { labels: ["important"] });
  });

  it("preserves other labels when folding", async () => {
    const user = userEvent.setup();
    renderActions(baseComment({ labels: ["important"] }));
    await user.click(screen.getByRole("button", { name: "comment actions" }));
    await user.click(within(screen.getByRole("menu")).getByText("Fold comment"));
    expect(updateComment).toHaveBeenCalledWith("comment-1", { labels: ["important", "fold"] });
  });

  it("does not crash when labels is undefined (guards missing array)", async () => {
    const user = userEvent.setup();
    renderActions(baseComment({ labels: undefined }));
    await user.click(screen.getByRole("button", { name: "comment actions" }));
    await user.click(within(screen.getByRole("menu")).getByText("Fold comment"));
    expect(updateComment).toHaveBeenCalledWith("comment-1", { labels: ["fold"] });
  });

  it("offers fold/unfold on another actor's comment (not author-gated)", async () => {
    const user = userEvent.setup();
    // comment authored by a different actor than the current user
    renderActions(baseComment({ actor: "agent-9", labels: ["fold"] }));
    await user.click(screen.getByRole("button", { name: "comment actions" }));
    const menu = screen.getByRole("menu");
    // author-only actions are hidden...
    expect(within(menu).queryByText("Edit")).not.toBeInTheDocument();
    expect(within(menu).queryByText("Delete")).not.toBeInTheDocument();
    // ...but unfold is still available
    expect(within(menu).getByText("Unfold comment")).toBeInTheDocument();
  });
});
