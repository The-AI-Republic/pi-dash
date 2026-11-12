// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-action side effects (parity/specs/issues.md §7, ISS-060–067):
// the deep link, the clipboard, the membership deletes, the restore and
// the dialogs each entry opens. The hook runs over a recording transport,
// so the test asserts the request rather than only the rendered menu.

import { QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { IssueListItem } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createQueryClient } from "../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { ArchiveWorkItemDialog } from "../lifecycle/ArchiveWorkItemDialog.js";
import { DeleteWorkItemDialog } from "../lifecycle/DeleteWorkItemDialog.js";
import { MoveWorkItemDialog } from "../lifecycle/MoveWorkItemDialog.js";
import { WorkItemCreateDialog } from "../components/WorkItemCreateDialog.js";
import {
  LIST_COPY_MESSAGE,
  REMOVE_FAILED_BODY,
  REMOVE_FAILED_TITLE,
  RESTORE_FAILED_TITLE,
  RESTORE_SUCCESS_TITLE,
  removeSuccessTitle,
} from "./messages.js";
import {
  copySeed,
  moveTargetsFrom,
  quickActionCreateContext,
  quickActionIcon,
  quickActionTarget,
  useQuickActions,
  useMenuStateGroup,
  type QuickActionTarget,
} from "./useQuickActions.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "223e4567-e89b-12d3-a456-4266141740ff";
const ISSUE = "123e4567-e89b-12d3-a456-42661417400a";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";
const ME = "623e4567-e89b-12d3-a456-426614174005";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: CYCLE,
    module_ids: [MODULE],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-03-01T00:00:00Z",
    updated_at: "2026-03-02T00:00:00Z",
    created_by: ME,
    updated_by: ME,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function target(overrides: Partial<QuickActionTarget> = {}): QuickActionTarget {
  return { ...quickActionTarget(row(), "WEB"), ...overrides };
}

interface Call {
  url: string;
  method: string;
  body: string;
}

interface Stub {
  /** Status for the endpoint fragment, e.g. "archive". */
  fail?: string;
  projects?: unknown[];
  states?: unknown[];
  /** Answer the signed-in user, so `useMe` resolves. */
  me?: boolean;
}

function transport(stub: Stub = {}) {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.endsWith("/states/")) {
      return json(
        200,
        stub.states ?? [
          {
            id: STATE,
            project_id: PROJECT,
            workspace_id: PROJECT,
            name: "Todo",
            color: "#000",
            group: "started",
            default: false,
            description: "",
            sequence: 1,
          },
        ]
      );
    }
    if (url.endsWith("/projects/") && method === "GET") {
      return json(
        200,
        stub.projects ?? [
          {
            id: PROJECT,
            name: "Web",
            identifier: "WEB",
            sort_order: null,
            logo_props: {},
            member_role: 20,
            intake_count: 0,
            archived_at: null,
            workspace: PROJECT,
            cycle_view: true,
            issue_views_view: true,
            module_view: true,
            estimate: null,
            page_view: true,
            inbox_view: true,
            is_default: false,
            guest_view_all_features: false,
            project_lead: null,
            network: 2,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            created_by: null,
            updated_by: null,
          },
        ]
      );
    }
    if (url.includes("/members/") && url.includes("/workspaces/")) return json(200, { results: [] });
    if (stub.me === true && url.endsWith("/api/users/me/")) {
      return json(200, {
        id: ME,
        email: "ada@example.test",
        username: "ada",
        first_name: "Ada",
        last_name: "L",
        display_name: "Ada L",
        avatar: "",
        avatar_url: null,
        cover_image: null,
        cover_image_url: null,
        date_joined: "2026-01-01T00:00:00Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "email",
        last_login_time: null,
      });
    }
    if (stub.fail !== undefined && url.includes(stub.fail)) return json(500, { error: "boom" });
    if (method === "DELETE") return json(204, {});
    if (method === "POST") return json(201, { message: "ok" });
    return json(200, { results: [] });
  }) as never);
  return calls;
}

function wrapper() {
  const queryClient = createQueryClient();
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function asOverlay(node: React.ReactElement | null | undefined): React.ReactElement | null {
  return node ?? null;
}

function dialogOf(state: ReturnType<typeof useQuickActions>): React.ReactElement<Record<string, unknown>> | null {
  const overlay = asOverlay(state?.overlays);
  if (overlay === null) return null;
  const children = React.Children.toArray(
    (overlay.props as { children: React.ReactNode }).children
  ) as React.ReactElement<Record<string, unknown>>[];
  return children.find((child) => typeof child.type !== "string") ?? null;
}

/** The dialog a state mounts right now; the tests assert on its props. */
function mountedDialog(state: ReturnType<typeof useQuickActions>): React.ReactElement<Record<string, unknown>> {
  const dialog = dialogOf(state);
  if (dialog === null) throw new Error("expected a mounted dialog");
  return dialog;
}

function renderMenu(input: Parameters<typeof useQuickActions>[0]) {
  return renderHook(() => useQuickActions(input), { wrapper: wrapper() });
}

const clipboard = { writeText: vi.fn() };

beforeEach(() => {
  vi.clearAllMocks();
  clipboard.writeText.mockResolvedValue(undefined);
  Object.defineProperty(globalThis.navigator, "clipboard", { value: clipboard, configurable: true });
  Object.defineProperty(globalThis, "open", { value: vi.fn(), configurable: true, writable: true });
});

afterEach(() => {
  resetAppClient();
});

describe("quickActionTarget", () => {
  it("keeps the row's identity, state, creator and archive flag", () => {
    const converted = quickActionTarget(row({ archived_at: "2026-03-01", is_epic: true }), "WEB");
    expect(converted).toMatchObject({
      id: ISSUE,
      name: "Fix login",
      sequenceId: 42,
      projectId: PROJECT,
      projectIdentifier: "WEB",
      stateId: STATE,
      createdBy: ME,
      archived: true,
      isEpic: true,
    });
    expect(converted.snapshot).toEqual(row({ archived_at: "2026-03-01", is_epic: true }));
  });
});

describe("copySeed / moveTargetsFrom / quickActionIcon", () => {
  it("names the copy after the source and keeps its fields", () => {
    expect(copySeed(target())).toMatchObject({ id: ISSUE, name: "Fix login (copy)", sequence_id: 42 });
  });

  it("seeds a copy from a target without a snapshot", () => {
    expect(copySeed({ ...target(), snapshot: undefined })).toEqual({
      id: ISSUE,
      name: "Fix login (copy)",
    });
  });

  it("offers joined, live projects with a code", () => {
    expect(
      moveTargetsFrom([
        { id: PROJECT, name: "Web", identifier: "WEB", member_role: 20, archived_at: null },
        { id: OTHER, name: "Old", identifier: "OLD", member_role: 20, archived_at: "2026-01-01" },
        { id: "x", name: "Outsider", identifier: "OUT", member_role: null, archived_at: null },
        { id: "y", name: "Blank", identifier: "", member_role: 20, archived_at: null },
      ])
    ).toEqual([{ id: PROJECT, name: "Web", identifier: "WEB" }]);
  });

  it("gives every entry a mark", () => {
    for (const key of [
      "edit",
      "copy",
      "openInNewTab",
      "copyLink",
      "move",
      "archive",
      "restore",
      "delete",
      "removeFromCycle",
      "removeFromModule",
    ] as const) {
      expect(quickActionIcon(key)).not.toBeNull();
    }
  });
});

describe("useQuickActions", () => {
  it("resolves the project standing and the row's state group into entries", async () => {
    transport();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await waitFor(() => expect(result.current?.entries.map((entry) => entry.label)).toContain("Edit"));
    expect(result.current?.entries.find((entry) => entry.key === "archive")?.disabled).toBe(true);
  });

  it("returns null for a surface with no target", () => {
    transport();
    const { result } = renderMenu(null);
    expect(result.current).toBeNull();
  });

  it("opens the detail address in a new tab", async () => {
    transport();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await act(async () => result.current?.handlers.run("openInNewTab"));
    expect(globalThis.open).toHaveBeenCalledWith(`/${SLUG}/browse/WEB-42/`, "_blank", "noopener,noreferrer");
  });

  it("copies the detail address and toasts", async () => {
    transport();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await act(async () => result.current?.handlers.run("copyLink"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link copied", undefined));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts the failure when the clipboard refuses", async () => {
    transport();
    clipboard.writeText.mockRejectedValue(new Error("Denied"));
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await act(async () => result.current?.handlers.run("copyLink"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the link", "Denied"));
  });

  it("opens the override address in a new tab when one is given (ARCH-010)", async () => {
    transport();
    const linkHref = `/${SLUG}/projects/${PROJECT}/archives/issues/${ISSUE}`;
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "archived", linkHref });
    await act(async () => result.current?.handlers.run("openInNewTab"));
    expect(globalThis.open).toHaveBeenCalledWith(linkHref, "_blank", "noopener,noreferrer");
  });

  it("copies the override address when one is given (ARCH-010)", async () => {
    transport();
    const linkHref = `/${SLUG}/projects/${PROJECT}/archives/issues/${ISSUE}`;
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "archived", linkHref });
    await act(async () => result.current?.handlers.run("copyLink"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link copied", undefined));
    expect(clipboard.writeText).toHaveBeenCalledWith(expect.stringContaining(linkHref));
  });

  it("opens the edit and copy forms as dialogs with the project standing", async () => {
    transport();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "cycle", cycleId: CYCLE });
    await act(async () => result.current?.handlers.run("edit"));
    expect(result.current?.overlays).not.toBeNull();
    await act(async () => result.current?.handlers.run("copy"));
    expect(result.current?.overlays).not.toBeNull();
    await act(async () => result.current?.handlers.run("archive"));
    expect(result.current?.overlays).not.toBeNull();
    await act(async () => result.current?.handlers.run("move"));
    expect(result.current?.overlays).not.toBeNull();
    await act(async () => result.current?.handlers.run("delete"));
    expect(result.current?.overlays).not.toBeNull();
  });

  it("removes a row from its cycle through the bridge delete", async () => {
    const calls = transport();
    const onChanged = vi.fn();
    const { result } = renderMenu({
      workspaceSlug: SLUG,
      target: target(),
      context: "cycle",
      cycleId: CYCLE,
      onChanged,
    });
    await act(async () => result.current?.handlers.run("removeFromCycle"));
    await waitFor(() =>
      expect(calls).toEqual(
        expect.arrayContaining([
          {
            url: `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/cycles/${CYCLE}/cycle-issues/${ISSUE}/`,
            method: "DELETE",
            body: "",
          },
        ])
      )
    );
    expect(notifySuccess).toHaveBeenCalledWith(removeSuccessTitle("cycle"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("removes a row from its module through the membership write", async () => {
    const calls = transport();
    const { result } = renderMenu({
      workspaceSlug: SLUG,
      target: target(),
      context: "module",
      moduleId: MODULE,
    });
    await act(async () => result.current?.handlers.run("removeFromModule"));
    await waitFor(() => {
      const post = calls.find((call) => call.method === "POST" && call.url.includes("/modules/"));
      expect(post?.url).toBe(`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/modules/`);
      expect(JSON.parse(post?.body ?? "{}")).toEqual({ modules: [], removed_modules: [MODULE] });
    });
    expect(notifySuccess).toHaveBeenCalledWith(removeSuccessTitle("module"));
  });

  it("reports a failed membership removal, and one with no context", async () => {
    transport({ fail: "/cycles/" });
    const { result, rerender } = renderMenu({
      workspaceSlug: SLUG,
      target: target(),
      context: "cycle",
      cycleId: CYCLE,
    });
    await act(async () => result.current?.handlers.run("removeFromCycle"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(REMOVE_FAILED_TITLE, REMOVE_FAILED_BODY));

    vi.mocked(notifyError).mockClear();
    rerender();
    const bare = renderMenu({ workspaceSlug: SLUG, target: target(), context: "module" });
    bare.result.current?.handlers.run("removeFromModule");
    expect(notifyError).toHaveBeenCalledWith(REMOVE_FAILED_TITLE, REMOVE_FAILED_BODY);
  });

  it("restores an archived row, then reports a refusal", async () => {
    const calls = transport();
    const onRestored = vi.fn();
    const { result } = renderMenu({
      workspaceSlug: SLUG,
      target: target({ archived: true }),
      context: "archived",
      onRestored,
    });
    await act(async () => result.current?.handlers.run("restore"));
    await waitFor(() =>
      expect(calls).toEqual(
        expect.arrayContaining([
          {
            url: `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/archive/`,
            method: "DELETE",
            body: "",
          },
        ])
      )
    );
    expect(notifySuccess).toHaveBeenCalledWith(RESTORE_SUCCESS_TITLE, expect.any(String));
    await waitFor(() => expect(onRestored).toHaveBeenCalledTimes(1));

    vi.mocked(notifySuccess).mockClear();
    resetAppClient();
    transport({ fail: "/archive/" });
    const failing = renderMenu({ workspaceSlug: SLUG, target: target({ archived: true }), context: "archived" });
    await act(async () => failing.result.current?.handlers.run("restore"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(RESTORE_FAILED_TITLE, expect.any(String)));
  });
});

describe("quickActionCreateContext", () => {
  it("prefills both memberships, then only the one the page carries", () => {
    expect(quickActionCreateContext(CYCLE, MODULE, "copy")).toEqual({ cycleId: CYCLE, moduleId: MODULE });
    expect(quickActionCreateContext(CYCLE, null, "edit")).toEqual({ cycleId: CYCLE });
    expect(quickActionCreateContext(undefined, MODULE, "copy")).toEqual({ moduleId: MODULE });
  });

  it("hands nothing to a form opened outside a create flow or with blank memberships", () => {
    expect(quickActionCreateContext(CYCLE, MODULE, "archive")).toBeUndefined();
    expect(quickActionCreateContext(CYCLE, MODULE, null)).toBeUndefined();
    expect(quickActionCreateContext(null, undefined, "edit")).toBeUndefined();
    expect(quickActionCreateContext("", "", "copy")).toBeUndefined();
  });
});

describe("quick-action dialog wiring", () => {
  const scope = { workspaceSlug: SLUG, projectId: PROJECT, issueId: ISSUE };

  beforeEach(() => {
    transport();
  });

  function mount(node: React.ReactElement): ReturnType<typeof render> {
    return render(
      <QueryClientProvider client={createQueryClient()}>
        <React.Fragment>{node}</React.Fragment>
      </QueryClientProvider>
    );
  }

  it("hands Make a copy the source row and its suffixed title (ISS-060)", async () => {
    transport();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await act(async () => result.current?.handlers.run("copy"));
    const overlay = result.current?.overlays;
    expect(overlay).not.toBeNull();
    const forms = React.Children.toArray((overlay as React.ReactElement<{ children: React.ReactNode }>).props.children);
    const form = forms.find((child) => React.isValidElement(child) && child.type === WorkItemCreateDialog);
    expect(form).toBeDefined();
    const seeded = (form as React.ReactElement<{ copySeed: { title: string; snapshot: unknown } }>).props.copySeed;
    expect(seeded.title).toBe("Fix login (copy)");
    expect(seeded.snapshot).toMatchObject({ id: ISSUE, name: "Fix login (copy)" });
  });

  it("closes the create and edit forms through the caller's open state", () => {
    const onChange = vi.fn();
    const edit = { kind: "issue" as const, id: ISSUE, projectId: PROJECT, snapshot: row() };
    for (const node of [
      <WorkItemCreateDialog key="new" open onOpenChange={onChange} workspaceSlug={SLUG} projects={[]} />,
      <WorkItemCreateDialog key="edit" open onOpenChange={onChange} workspaceSlug={SLUG} projects={[]} edit={edit} />,
    ]) {
      const view = mount(node);
      fireEvent.keyDown(document.body, { key: "Escape" });
      view.unmount();
    }
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("closes the confirmation dialogs through the caller's open state", () => {
    const onChange = vi.fn();
    const views = [
      mount(
        <ArchiveWorkItemDialog open onOpenChange={onChange} scope={scope} projectIdentifier="WEB" sequenceId={42} />
      ),
      mount(
        <DeleteWorkItemDialog
          open
          onOpenChange={onChange}
          scope={scope}
          projectIdentifier="WEB"
          sequenceId={42}
          viewerId={ME}
          creatorId={ME}
          projectRole={20}
        />
      ),
      mount(
        <MoveWorkItemDialog
          open
          onOpenChange={onChange}
          scope={scope}
          projects={[{ id: OTHER, name: "Betel", identifier: "BET" }]}
          identifierOf={() => "BET"}
        />
      ),
    ];
    for (const view of views) {
      fireEvent.keyDown(document.body, { key: "Escape" });
      view.unmount();
    }
    expect(onChange).toHaveBeenCalledWith(false);
  });
});

describe("quick-action overlays", () => {
  const CASES = [
    ["copy", WorkItemCreateDialog],
    ["archive", ArchiveWorkItemDialog],
    ["delete", DeleteWorkItemDialog],
    ["move", MoveWorkItemDialog],
  ] as const;

  for (const [key, type] of CASES) {
    it(`mounts only the ${key} dialog, and dismisses it once the dialog asks to close (ISS-060/066)`, async () => {
      transport();
      const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
      await act(async () => result.current?.handlers.run(key));
      const dialog = mountedDialog(result.current as ReturnType<typeof useQuickActions>);
      expect(dialog.type).toBe(type);
      const onOpenChange = dialog.props["onOpenChange"] as (next: boolean) => void;
      act(() => onOpenChange(true));
      expect(mountedDialog(result.current as ReturnType<typeof useQuickActions>).type).toBe(type);
      act(() => onOpenChange(false));
      expect(dialogOf(result.current as ReturnType<typeof useQuickActions>)).toBeNull();
    });
  }

  it("closes the create form and refreshes once a copy is saved (ISS-060)", async () => {
    transport();
    const onChanged = vi.fn();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project", onChanged });
    await act(async () => result.current?.handlers.run("copy"));
    const onCreated = mountedDialog(result.current as ReturnType<typeof useQuickActions>).props[
      "onCreated"
    ] as () => void;
    await act(async () => onCreated());
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(dialogOf(result.current as ReturnType<typeof useQuickActions>)).toBeNull();
  });

  it("closes the archive confirmation after the write lands and refreshes (ISS-061)", async () => {
    transport();
    const onChanged = vi.fn();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project", onChanged });
    await act(async () => result.current?.handlers.run("archive"));
    await act(async () =>
      (mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["onArchived"] as () => void)()
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(dialogOf(result.current as ReturnType<typeof useQuickActions>)).toBeNull();
  });

  it("names an epic's dialogs after the epic and falls back when the viewer is unknown (ISS-060/066)", async () => {
    transport();
    const { result } = renderMenu({
      workspaceSlug: SLUG,
      target: target({ isEpic: true, createdBy: null }),
      context: "detail",
    });
    await act(async () => result.current?.handlers.run("archive"));
    expect(mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["kind"]).toBe("epic");
    act(() =>
      (
        mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["onOpenChange"] as (
          n: boolean
        ) => void
      )(false)
    );
    await act(async () => result.current?.handlers.run("delete"));
    const dialog = mountedDialog(result.current as ReturnType<typeof useQuickActions>);
    expect(dialog.props["kind"]).toBe("epic");
    expect(dialog.props["creatorId"]).toBeNull();
  });

  it("hands the delete confirmation the signed-in viewer (ISS-066)", async () => {
    transport({ me: true });
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "detail" });
    await act(async () => result.current?.handlers.run("delete"));
    await waitFor(() =>
      expect(mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["viewerId"]).toBe(ME)
    );
  });

  it("opens the edit form from the caller's row, or from its key when it holds none (ISS-060)", async () => {
    transport();
    const held = renderMenu({ workspaceSlug: SLUG, target: target(), context: "project" });
    await act(async () => held.result.current?.handlers.run("edit"));
    const fromRow = mountedDialog(held.result.current as ReturnType<typeof useQuickActions>);
    expect((fromRow.props["edit"] as { snapshot: { name: string } }).snapshot.name).toBe("Fix login");

    resetAppClient();
    transport();
    const bare = renderMenu({
      workspaceSlug: SLUG,
      target: { ...target(), snapshot: undefined },
      context: "project",
    });
    await act(async () => bare.result.current?.handlers.run("edit"));
    const fromKey = mountedDialog(bare.result.current as ReturnType<typeof useQuickActions>);
    expect((fromKey.props["edit"] as { snapshot: { name: string } }).snapshot.name).toBe("Fix login");
    expect((fromKey.props["edit"] as { snapshot: { id: string } }).snapshot.id).toBe(ISSUE);
  });

  it("closes the delete confirmation after the delete lands and refreshes (ISS-066)", async () => {
    transport();
    const onChanged = vi.fn();
    const { result } = renderMenu({ workspaceSlug: SLUG, target: target(), context: "detail", onChanged });
    await act(async () => result.current?.handlers.run("delete"));
    await act(async () =>
      (mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["onDeleted"] as () => void)()
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(dialogOf(result.current as ReturnType<typeof useQuickActions>)).toBeNull();
  });

  it("runs the detail follow-ups after a landed archive or delete (ISS-162)", async () => {
    transport();
    const onArchived = vi.fn();
    const onDeleted = vi.fn();
    const { result } = renderMenu({
      workspaceSlug: SLUG,
      target: target(),
      context: "detail",
      onArchived,
      onDeleted,
    });
    await act(async () => result.current?.handlers.run("archive"));
    await act(async () =>
      (mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["onArchived"] as () => void)()
    );
    expect(onArchived).toHaveBeenCalledTimes(1);
    await act(async () => result.current?.handlers.run("delete"));
    await act(async () =>
      (mountedDialog(result.current as ReturnType<typeof useQuickActions>).props["onDeleted"] as () => void)()
    );
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });
});

describe("useMenuStateGroup", () => {
  it("prefers a group the caller already knows", () => {
    transport();
    const { result } = renderHook(
      () => useMenuStateGroup(SLUG, { projectId: PROJECT, stateId: STATE, stateGroup: "completed" }),
      { wrapper: wrapper() }
    );
    expect(result.current).toBe("completed");
  });

  it("answers from the picker states for the row's state id", async () => {
    transport({
      states: [
        {
          id: STATE,
          project_id: PROJECT,
          workspace_id: PROJECT,
          name: "Cancelled",
          color: "#000",
          group: "cancelled",
          default: false,
          description: "",
          sequence: 1,
        },
      ],
    });
    const { result } = renderHook(() => useMenuStateGroup(SLUG, { projectId: PROJECT, stateId: STATE }), {
      wrapper: wrapper(),
    });
    await waitFor(() => expect(result.current).toBe("cancelled"));
  });

  it("stays unknown for a row without a state", () => {
    transport();
    const { result } = renderHook(() => useMenuStateGroup(SLUG, { projectId: PROJECT, stateId: null }), {
      wrapper: wrapper(),
    });
    expect(result.current).toBeNull();
  });

  it("keeps the list link message stable", () => {
    expect(LIST_COPY_MESSAGE).toContain("link copied to clipboard");
  });
});
