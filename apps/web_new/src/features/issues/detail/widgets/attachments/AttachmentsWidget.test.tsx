// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";
import { resetRegisteredStores } from "../../../../../core/session/index.js";

vi.mock("../../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    notifyError: vi.fn(),
    notifySuccess: vi.fn(),
  };
});

import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { AttachmentsWidget } from "./AttachmentsWidget.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const ASSET = "423e4567-e89b-12d3-a456-426614174003";
const USER = "523e4567-e89b-12d3-a456-426614174004";
const WORKSPACE = "623e4567-e89b-12d3-a456-426614174005";

function attachmentRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ASSET,
    attributes: { name: "plan.png", type: "image/png", size: 512 },
    asset: "assets/plan.png",
    user: USER,
    workspace: WORKSPACE,
    draft_issue: null,
    project: PROJECT,
    issue: ISSUE,
    comment: null,
    page: null,
    entity_type: "ISSUE_ATTACHMENT",
    entity_identifier: ISSUE,
    is_deleted: false,
    is_archived: false,
    external_id: null,
    external_source: null,
    size: 512,
    is_uploaded: true,
    storage_metadata: null,
    created_at: "2026-10-18T00:00:00Z",
    updated_at: "2026-10-18T00:00:00Z",
    created_by: USER,
    updated_by: null,
    deleted_at: null,
    asset_url: `/api/assets/v2/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/attachments/${ASSET}/`,
    ...overrides,
  };
}

interface StubOptions {
  rows?: Record<string, unknown>[];
  listStatus?: number;
  postStatus?: number;
  patchStatus?: number;
  deleteStatus?: number;
  bytesOk?: boolean;
  limit?: number | undefined;
  hangList?: boolean;
}

function stubTransport(options: StubOptions = {}) {
  const calls: { url: string; method: string; body: string }[] = [];
  const byteCalls: string[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.endsWith("/api/instances/")) {
      return json(200, {
        config: {
          enable_signup: true,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: true,
          is_email_password_enabled: true,
          is_smtp_configured: false,
          is_self_managed: true,
          instance_changelog_url: "",
          ...(options.limit === undefined ? {} : { file_size_limit: options.limit }),
        },
      });
    }
    if (url.endsWith(`/api/workspaces/${SLUG}/members/`)) {
      return json(200, [
        {
          id: USER,
          member: {
            id: USER,
            first_name: "Ada",
            last_name: "L",
            avatar: "",
            avatar_url: null,
            display_name: "Ada L",
            is_bot: false,
          },
          role: 15,
          is_active: true,
        },
      ]);
    }
    if (url.includes("/attachments/") && url.endsWith("/attachments/")) {
      const method = init.method ?? "GET";
      if (method === "GET") {
        if (options.hangList) return new Promise<never>(() => undefined);
        return json(
          options.listStatus ?? 200,
          options.listStatus && options.listStatus >= 400 ? {} : (options.rows ?? [])
        );
      }
      if (method === "POST") {
        if (options.postStatus !== undefined && options.postStatus >= 400) {
          return json(options.postStatus, { error: "bad type" });
        }
        return json(201, {
          upload_data: { url: "https://storage.test/upload", fields: { key: "k" } },
          asset_id: ASSET,
          attachment: attachmentRow({ attributes: { name: "new.png", type: "image/png", size: 512 } }),
          asset_url: null,
        });
      }
    }
    if (url.includes(`/attachments/${ASSET}/`)) {
      const method = init.method ?? "GET";
      if (method === "PATCH") return json(options.patchStatus ?? 204, {});
      if (method === "DELETE") return json(options.deleteStatus ?? 204, {});
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url}`);
  }) as never);
  vi.stubGlobal("fetch", async (url: string) => {
    byteCalls.push(url);
    return { ok: options.bytesOk ?? true, status: options.bytesOk === false ? 500 : 204 };
  });
  return { calls, byteCalls };
}

function setup(options: StubOptions = {}, overrides: Partial<React.ComponentProps<typeof AttachmentsWidget>> = {}) {
  stubTransport(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable {...overrides} />
    </QueryClientProvider>
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => {
  vi.unstubAllGlobals();
  resetAppClient();
  resetRegisteredStores();
});

describe("AttachmentsWidget list", () => {
  it("renders rows with name, human size and a served new-tab link", async () => {
    setup({ rows: [attachmentRow()] });
    const link = await screen.findByRole("link", { name: /plan\.png/ });
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute(
      "href",
      `https://api.test/api/assets/v2/workspaces/${SLUG}/projects/${PROJECT}/issues/${ISSUE}/attachments/${ASSET}/`
    );
    expect(link).toHaveTextContent("1 KB");
    expect(screen.getByRole("button", { name: /Actions for plan\.png/ })).toBeInTheDocument();
  });

  it("renders no section chrome on an empty issue", async () => {
    setup();
    expect(await screen.findByLabelText("Choose an attachment")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Attachments/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Drop one file here/)).not.toBeInTheDocument();
  });

  it("opens the picker through the shared handle", async () => {
    const ref = React.createRef<import("./AttachmentsWidget.js").AttachmentsWidgetHandle>();
    stubTransport();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable ref={ref} />
      </QueryClientProvider>
    );
    const input = await screen.findByLabelText("Choose an attachment");
    const click = vi.spyOn(input, "click");
    ref.current?.openPicker();
    expect(click).toHaveBeenCalled();
  });

  it("reads only without attach, menu or delete", async () => {
    setup({ rows: [attachmentRow()] }, { editable: false });
    expect(await screen.findByRole("link", { name: /plan\.png/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Actions for/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Attachments/ })).toBeInTheDocument();
  });

  it("collapses rows from the section header", async () => {
    const user = userEvent.setup();
    setup({ rows: [attachmentRow()] });
    await screen.findByRole("link", { name: /plan\.png/ });
    await user.click(screen.getByRole("button", { name: /Attachments/ }));
    expect(screen.queryByRole("link", { name: /plan\.png/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Attachments/ }));
    expect(screen.getByRole("link", { name: /plan\.png/ })).toBeInTheDocument();
  });
});

describe("AttachmentsWidget upload", () => {
  it("uploads one picked file through the three legs and toasts success", async () => {
    const { byteCalls } = stubTransport();
    const options = { rows: [] as Record<string, unknown>[] };
    void options;
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable />
      </QueryClientProvider>
    );
    const input = await screen.findByLabelText("Choose an attachment");
    const file = new File([new Uint8Array(512)], "new.png", { type: "image/png" });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Attachment uploaded"));
    expect(byteCalls).toEqual(["https://storage.test/upload"]);
  });

  it("rejects a multi-file drop with the single-file notice", async () => {
    setup();
    const section = await screen.findByRole("region", { name: "Attachments" });
    const dataTransfer = {
      files: [
        new File([new Uint8Array(1)], "a.png", { type: "image/png" }),
        new File([new Uint8Array(1)], "b.png", { type: "image/png" }),
      ],
    };
    fireEvent.drop(section, { dataTransfer });
    expect(notifyError).toHaveBeenCalledWith("Only one file can be attached at a time.");
  });

  it("rejects an oversize file with the configured ceiling", async () => {
    setup({ limit: 1024 });
    await waitFor(() =>
      expect(screen.getByRole("region", { name: "Attachments" })).toHaveAttribute("data-file-size-limit", "1024")
    );
    const input = screen.getByLabelText("Choose an attachment");
    const file = new File([new Uint8Array(1025)], "big.png", { type: "image/png" });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Files can't be larger than 0.0009765625MB."));
  });

  it("falls back to 5 MiB when the instance omits the ceiling", async () => {
    setup({ limit: undefined });
    await waitFor(() =>
      expect(screen.getByRole("region", { name: "Attachments" })).toHaveAttribute(
        "data-file-size-limit",
        String(5 * 1024 * 1024)
      )
    );
    const input = screen.getByLabelText("Choose an attachment");
    const file = new File([new Uint8Array(5 * 1024 * 1024 + 1)], "big.png", { type: "image/png" });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Files can't be larger than 5MB."));
  });

  it("surfaces a refused metadata leg as a failed attach", async () => {
    setup({ postStatus: 400 });
    const input = await screen.findByLabelText("Choose an attachment");
    fireEvent.change(input, { target: { files: [new File([new Uint8Array(2)], "x.txt", { type: "text/plain" })] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("The file could not be attached."));
  });

  it("surfaces a refused byte leg as a failed attach", async () => {
    setup({ bytesOk: false });
    const input = await screen.findByLabelText("Choose an attachment");
    fireEvent.change(input, { target: { files: [new File([new Uint8Array(2)], "x.png", { type: "image/png" })] } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("The file could not be attached."));
  });
});

describe("AttachmentsWidget delete", () => {
  it("confirms with the base name and permanence warning, then deletes", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport({ rows: [attachmentRow()] });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const mounted = render(
      <QueryClientProvider client={queryClient}>
        <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable />
      </QueryClientProvider>
    );
    await user.click(await screen.findByRole("button", { name: /Actions for plan\.png/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete attachment" });
    expect(dialog).toHaveTextContent("plan");
    expect(dialog).toHaveTextContent("permanently removed");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Attachment removed"));
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/attachments/${ASSET}/`))).toBe(true);
    mounted.unmount();
  });

  it("cancels without a request", async () => {
    const user = userEvent.setup();
    const { calls } = stubTransport({ rows: [attachmentRow()] });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable />
      </QueryClientProvider>
    );
    await user.click(await screen.findByRole("button", { name: /Actions for plan\.png/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }));
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
  });

  it("reports a refused delete", async () => {
    const user = userEvent.setup();
    stubTransport({ rows: [attachmentRow()], deleteStatus: 403 });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <AttachmentsWidget workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} editable />
      </QueryClientProvider>
    );
    await user.click(await screen.findByRole("button", { name: /Actions for plan\.png/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't remove the attachment"));
  });
});
