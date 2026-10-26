// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested page tree (parity/specs/pages.md §12.2, PAGE-085/087): pages with
// children carry an expand control and a visible count, expanding reads
// that parent's children and indents them, a collapsed parent shows only
// its own line, and the pure drag helpers resolve a dragged id and the
// container as the top level.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: vi.fn(() => ({ settle: vi.fn() })),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));
vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: () => true,
  setFavoritesSectionOpen: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { listPages } from "@pidash/api-client";
import { PAGES_PROJECT_ID, PAGES_USER_ID, jsonResponse, makePageRow } from "../testing.js";
import { childCountLabel, draggedPageId, dropTargetId, expandLabel, PAGE_DRAG_MIME, PageTree } from "./PageTree.js";
import { resetPageExpansion } from "../lib/pageTreeState.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { PAGE_MOVE_REFUSED_TITLE, pageMoveRefusalMessage } from "../lib/pageNested.js";

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listPages: vi.fn(),
}));

const listPagesMock = vi.mocked(listPages);

const OWNER = { id: PAGES_USER_ID, name: "Ada Byron", avatarUrl: null };
const PARENT = "323e4567-e89b-12d3-a456-426614174101";
const CHILD = "323e4567-e89b-12d3-a456-426614174102";
const GRAND = "323e4567-e89b-12d3-a456-426614174103";

const parent = makePageRow({ id: PARENT, name: "Handbook", sub_pages_count: 1, owned_by: PAGES_USER_ID });
const child = makePageRow({
  id: CHILD,
  name: "Policies",
  parent: PARENT,
  parent_id: PARENT,
  sub_pages_count: 1,
  owned_by: PAGES_USER_ID,
});
const grand = makePageRow({
  id: GRAND,
  name: "Leave",
  parent: CHILD,
  parent_id: CHILD,
  sub_pages_count: 0,
  owned_by: PAGES_USER_ID,
});

function renderTree(overrides: Partial<React.ComponentProps<typeof PageTree>> = {}) {
  const handlers = {
    onCreateSubPage: vi.fn(),
    onOpenInNewTab: vi.fn(),
    onCopyLink: vi.fn(),
    onDuplicate: vi.fn(),
    onMovePage: vi.fn(),
    onToggleArchive: vi.fn(),
    onRequestMove: vi.fn(),
  };
  const props: React.ComponentProps<typeof PageTree> = {
    workspaceSlug: "acme",
    projectId: PAGES_PROJECT_ID,
    rows: [parent],
    pool: [parent, child, grand],
    tab: "public",
    sort: { key: "name", direction: "ascending" },
    showStar: false,
    canDuplicate: true,
    currentUserId: PAGES_USER_ID,
    viewerIsAdmin: false,
    canMove: true,
    canMoveProject: false,
    owners: [OWNER],
    ...handlers,
    ...overrides,
  };
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <PageTree {...props} />
    </QueryClientProvider>
  );
  return { ...rendered, handlers, props, queryClient };
}

beforeEach(() => {
  listPagesMock.mockReset();
  resetPageExpansion();
  createAppClient("https://api.test", (async (url: string) => jsonResponse(url, 200, {})) as never);
});

afterEach(() => {
  resetAppClient();
  resetPageExpansion();
});

describe("tree helpers", () => {
  it("names the expand control by title and state", () => {
    expect(expandLabel("Handbook", false)).toBe("Expand sub-pages of Handbook");
    expect(expandLabel("Handbook", true)).toBe("Collapse sub-pages of Handbook");
  });

  it("labels the count in the singular and plural", () => {
    expect(childCountLabel(1)).toBe("1 sub-page");
    expect(childCountLabel(3)).toBe("3 sub-pages");
  });

  it("reads a trimmed dragged id and treats a blank or absent transfer as none", () => {
    const transfer = (value: string) => ({ getData: (type: string) => (type === PAGE_DRAG_MIME ? value : "") });
    expect(draggedPageId(transfer(" abc "))).toBe("abc");
    expect(draggedPageId(transfer("   "))).toBeNull();
    expect(draggedPageId(null)).toBeNull();
  });

  it("resolves the container drop to the top level and keeps a row id otherwise", () => {
    expect(dropTargetId(undefined)).toBeNull();
    expect(dropTargetId(null)).toBeNull();
    expect(dropTargetId("page-1")).toBe("page-1");
  });
});

describe("PageTree", () => {
  it("shows a child count and an expand control for a parent with children", () => {
    renderTree();
    expect(screen.getByTestId("pages-tree")).toBeInTheDocument();
    expect(screen.getByTestId("page-tree-count")).toHaveTextContent("1");
    expect(screen.getByTestId("page-tree-count")).toHaveAccessibleName("1 sub-page");
    expect(screen.getByTestId("page-tree-expand")).toHaveAccessibleName("Expand sub-pages of Handbook");
    expect(screen.getAllByTestId("page-tree-item")).toHaveLength(1);
  });

  it("reads one parent's children on expand and indents them recursively", async () => {
    listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
      if (query?.parent === PARENT) return [child];
      if (query?.parent === CHILD) return [grand];
      return [];
    });
    renderTree();
    fireEvent.click(screen.getByTestId("page-tree-expand"));
    await waitFor(() => expect(screen.getByText("Policies")).toBeInTheDocument());
    expect(screen.getByText("Policies").closest('[data-testid="page-tree-item"]')).toHaveAttribute("data-depth", "1");

    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Policies" }));
    await waitFor(() => expect(screen.getByText("Leave")).toBeInTheDocument());
    expect(screen.getByText("Leave").closest('[data-testid="page-tree-item"]')).toHaveAttribute("data-depth", "2");
    expect(screen.getByRole("button", { name: "Collapse sub-pages of Handbook" })).toBeInTheDocument();
  });

  it("collapses a parent back to its own line", async () => {
    listPagesMock.mockResolvedValue([child]);
    renderTree();
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Handbook" }));
    await waitFor(() => expect(screen.getByText("Policies")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Collapse sub-pages of Handbook" }));
    expect(screen.queryByText("Policies")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("page-tree-item")).toHaveLength(1);
  });

  it("offers sub-page creation and the nested move only where allowed", async () => {
    renderTree();
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Handbook" }));
    expect(await screen.findByRole("menuitem", { name: "Add sub-page" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Move to…" })).toBeInTheDocument();
  });

  it("hides the nested move from a member who does not own the page", async () => {
    renderTree({ currentUserId: "523e4567-e89b-12d3-a456-426614174199", viewerIsAdmin: false, canMove: true });
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Handbook" }));
    expect(await screen.findByRole("menuitem", { name: "Add sub-page" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Move to…" })).not.toBeInTheDocument();
  });

  it("moves a dragged row onto another page", () => {
    const other = makePageRow({ id: "323e4567-e89b-12d3-a456-426614174104", name: "Other" });
    const onMovePage = vi.fn();
    renderTree({ onMovePage, rows: [parent, other], pool: [parent, other] });
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    const items = screen.getAllByTestId("page-tree-item");
    fireEvent.dragStart(items[0] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.drop(items[1] as HTMLElement, { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(parent, other.id);
  });

  it("moves a loaded child row, which is not in the top-level pool", async () => {
    listPagesMock.mockResolvedValue([child]);
    const other = makePageRow({ id: "323e4567-e89b-12d3-a456-426614174105", name: "Other" });
    const onMovePage = vi.fn();
    renderTree({ onMovePage, rows: [parent, other], pool: [parent, other] });
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Handbook" }));
    await screen.findByText("Policies");
    const source = screen.getByText("Policies").closest('[data-testid="page-tree-item"]') as HTMLElement;
    const target = screen.getByText("Other").closest('[data-testid="page-tree-item"]') as HTMLElement;
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    fireEvent.dragStart(source, { dataTransfer });
    fireEvent.dragOver(target, { dataTransfer });
    fireEvent.drop(target, { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(child, other.id);
  });

  it("drops onto a colleague-owned page instead of falling through to the top level", () => {
    const colleague = "523e4567-e89b-12d3-a456-426614174198";
    const other = makePageRow({ id: "323e4567-e89b-12d3-a456-426614174106", name: "Theirs", owned_by: colleague });
    const onMovePage = vi.fn();
    renderTree({ onMovePage, rows: [parent, other], pool: [parent, other] });
    const items = screen.getAllByTestId("page-tree-item");
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    fireEvent.dragStart(items[0] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.drop(items[1] as HTMLElement, { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(parent, other.id);
    expect(onMovePage).not.toHaveBeenCalledWith(parent, null);
  });

  it("sorts each child list by the active sort with favourites first", async () => {
    const alpha = makePageRow({
      id: "323e4567-e89b-12d3-a456-426614174107",
      name: "Alpha",
      parent: PARENT,
      parent_id: PARENT,
    });
    const beta = makePageRow({
      id: "323e4567-e89b-12d3-a456-426614174108",
      name: "Beta",
      parent: PARENT,
      parent_id: PARENT,
      is_favorite: true,
    });
    listPagesMock.mockResolvedValue([alpha, beta]);
    renderTree({ sort: { key: "name", direction: "ascending" } });
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Handbook" }));
    await screen.findByText("Beta");
    expect(screen.getAllByTestId("page-tree-item").map((item) => item.textContent)).toEqual([
      expect.stringContaining("Handbook"),
      expect.stringContaining("Beta"),
      expect.stringContaining("Alpha"),
    ]);
  });

  it("drops a row on the container at the top level after hovering another row", () => {
    const onMovePage = vi.fn();
    const sibling = makePageRow({ id: GRAND, name: "Travel", owned_by: PAGES_USER_ID });
    renderTree({ onMovePage, rows: [parent, sibling], pool: [parent, sibling] });
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    const items = screen.getAllByTestId("page-tree-item");
    fireEvent.dragStart(items[0] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(screen.getByTestId("pages-tree"), { dataTransfer });
    fireEvent.drop(screen.getByTestId("pages-tree"), { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(parent, null);
  });

  it("does not render an archived child in a live tab even when the parent read returns it", async () => {
    const archivedChild = makePageRow({
      id: CHILD,
      name: "Shelved policy",
      parent: PARENT,
      parent_id: PARENT,
      archived_at: "2026-10-20",
      owned_by: PAGES_USER_ID,
    });
    listPagesMock.mockResolvedValue([archivedChild]);
    renderTree({ tab: "public", pool: [parent, archivedChild] });
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Handbook" }));
    await waitFor(() => expect(listPagesMock).toHaveBeenCalled());
    expect(screen.queryByRole("link", { name: "Shelved policy" })).not.toBeInTheDocument();
  });

  it("keeps the archived row expandable and counts the shelved children it loads", async () => {
    const archivedParent = makePageRow({
      id: PARENT,
      name: "Shelved handbook",
      sub_pages_count: 0,
      archived_at: "2026-10-20",
      owned_by: PAGES_USER_ID,
    });
    const archivedChild = makePageRow({
      id: CHILD,
      name: "Shelved policy",
      parent: PARENT,
      parent_id: PARENT,
      archived_at: "2026-10-20",
      owned_by: PAGES_USER_ID,
    });
    listPagesMock.mockResolvedValue([archivedChild]);
    renderTree({ tab: "archived", rows: [archivedParent], pool: [archivedParent], includeArchivedChildren: true });
    expect(screen.queryByTestId("page-tree-count")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Shelved handbook" }));
    await screen.findByRole("link", { name: "Shelved policy" });
    expect(screen.getByTestId("page-tree-count")).toHaveTextContent("1");
    expect(listPagesMock).toHaveBeenCalledWith(expect.anything(), "acme", PAGES_PROJECT_ID, {
      parent: PARENT,
      includeArchived: true,
    });
  });

  it("drops the expand control after an archived parent's read proves it empty", async () => {
    const archivedLeaf = makePageRow({
      id: PARENT,
      name: "Shelved leaf",
      sub_pages_count: 0,
      archived_at: "2026-10-20",
      owned_by: PAGES_USER_ID,
    });
    listPagesMock.mockResolvedValue([]);
    renderTree({ tab: "archived", rows: [archivedLeaf], pool: [archivedLeaf], includeArchivedChildren: true });
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-pages of Shelved leaf" }));
    await waitFor(() => expect(listPagesMock).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Collapse sub-pages of Shelved leaf" })).not.toBeInTheDocument()
    );
  });

  it("marks the container as a drop target while a legal drag passes over it", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage });
    const container = screen.getByTestId("pages-tree");
    fireEvent.dragOver(container, { dataTransfer: { getData: () => "", setData: vi.fn() } });
    expect(container).toBeInTheDocument();
    expect(onMovePage).not.toHaveBeenCalled();
  });

  it("drops on the container to send a page back to the top level", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage });
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    const rootItem = screen.getByTestId("page-tree-item");
    fireEvent.dragStart(rootItem, { dataTransfer });
    fireEvent.drop(screen.getByTestId("pages-tree"), { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(parent, null);
  });

  it("ignores drags when the caller may not move pages", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage, canMove: false });
    const dataTransfer = { getData: () => PARENT, setData: vi.fn(), effectAllowed: "" };
    fireEvent.dragOver(screen.getByTestId("pages-tree"), { dataTransfer });
    fireEvent.drop(screen.getByTestId("pages-tree"), { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
    expect(screen.queryByRole("menuitem", { name: "Move to…" })).not.toBeInTheDocument();
  });

  it("keeps the row highlight stable on repeated dragOver events", () => {
    const onMovePage = vi.fn();
    const sibling = makePageRow({ id: GRAND, name: "Travel", owned_by: PAGES_USER_ID });
    renderTree({ onMovePage, rows: [parent, sibling], pool: [parent, sibling] });
    const items = screen.getAllByTestId("page-tree-item");
    const dataTransfer = { getData: () => PARENT, setData: vi.fn(), effectAllowed: "" };
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.drop(items[1] as HTMLElement, { dataTransfer });
    expect(onMovePage).toHaveBeenCalledWith(parent, GRAND);
  });

  it("refuses a drop of a row onto itself", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage });
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    const rootItem = screen.getByTestId("page-tree-item");
    fireEvent.dragStart(rootItem, { dataTransfer });
    fireEvent.drop(rootItem, { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
    expect(notifyError).toHaveBeenCalledWith(PAGE_MOVE_REFUSED_TITLE, pageMoveRefusalMessage("self"));
  });

  it("refuses a drop onto an archived page with the archived message", () => {
    const shelved = makePageRow({
      id: "323e4567-e89b-12d3-a456-426614174109",
      name: "Shelved",
      archived_at: "2026-10-20",
    });
    const onMovePage = vi.fn();
    renderTree({ onMovePage, rows: [parent, shelved], pool: [parent, shelved] });
    const items = screen.getAllByTestId("page-tree-item");
    const transfer: Record<string, string> = {};
    const dataTransfer = {
      getData: (type: string) => transfer[type] ?? "",
      setData: (type: string, value: string) => {
        transfer[type] = value;
      },
      effectAllowed: "",
    };
    fireEvent.dragStart(items[0] as HTMLElement, { dataTransfer });
    fireEvent.dragOver(items[1] as HTMLElement, { dataTransfer });
    fireEvent.drop(items[1] as HTMLElement, { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
    expect(notifyError).toHaveBeenCalledWith(PAGE_MOVE_REFUSED_TITLE, pageMoveRefusalMessage("archived"));
  });

  it("ignores a drag that carries no page id", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage });
    const dataTransfer = { getData: () => "", setData: vi.fn(), effectAllowed: "" };
    fireEvent.drop(screen.getByTestId("page-tree-item"), { dataTransfer });
    fireEvent.drop(screen.getByTestId("pages-tree"), { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
  });

  it("ignores a drop whose dragged page is not loaded", () => {
    const onMovePage = vi.fn();
    renderTree({ onMovePage });
    const dataTransfer = {
      getData: () => "323e4567-e89b-12d3-a456-426614174999",
      setData: vi.fn(),
      effectAllowed: "",
    };
    fireEvent.drop(screen.getByTestId("pages-tree"), { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
  });

  it("does not let a colleague drag or drop your row", () => {
    const colleague = "523e4567-e89b-12d3-a456-426614174197";
    const onMovePage = vi.fn();
    renderTree({ onMovePage, currentUserId: colleague, viewerIsAdmin: false, canMove: true });
    const rootItem = screen.getByTestId("page-tree-item");
    expect(rootItem).not.toHaveAttribute("draggable", "true");
    const dataTransfer = { getData: () => PARENT, setData: vi.fn(), effectAllowed: "" };
    fireEvent.drop(rootItem, { dataTransfer });
    expect(onMovePage).not.toHaveBeenCalled();
  });
});
