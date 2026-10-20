// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General tab (parity/specs/project-settings-general.md §PSET-005–009): the
// read view and its skeleton, the admin save with field rules, the live
// availability precheck and both write-time conflicts, the inert non-admin
// form, the out-of-band repository bind, the execution policy math and the
// two emblem surfaces (mark picker and user-asset upload).
// The app client is a call-recording stub; auth and toasts are mocked so a
// case pins one outcome at a time.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";

const harness = vi.hoisted(() => ({ role: 20 as number }));
const notify = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: notify.success,
  notifyError: notify.error,
  // The tab resolves uploaded emblems against the configured API origin.
  getToastManager: () => ({ add: () => undefined }),
}));

vi.mock("../components/ProjectSettingsLayout.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectSettings: () => ({ role: harness.role }),
}));

import { generalKeys } from "./api/queries.js";
import { authKeys } from "../../auth/index.js";
import {
  PROJECT_SAVED_MESSAGE,
  PROJECT_SAVED_TITLE,
  REPO_BIND_FAILED_TITLE,
  REPO_BOUND_TITLE,
} from "./api/mutations.js";
import {
  BASE_BRANCH_CHARS_MESSAGE,
  BUDGET_REQUIRED_MESSAGE,
  DESCRIPTION_PLACEHOLDER,
  IDENTIFIER_TAKEN_MESSAGE,
  IDENTIFIER_UNAVAILABLE_MESSAGE,
  NAME_REQUIRED_MESSAGE,
  REPO_EMPTY_MESSAGE,
  REPO_EMPTY_TITLE,
} from "./rules.js";
import { ProjectSettingsGeneralTab } from "./GeneralTabScreen.js";

const UUID = "223e4567-e89b-12d3-a456-426614174001";
const CREATED_AT = "2026-01-02T03:04:05Z";
const PROJECT_PATH = `/api/workspaces/acme/projects/${UUID}/`;
const REPOSITORY_PATH = `/api/workspaces/acme/projects/${UUID}/repository/`;
const BIND_PATH = `/api/workspaces/acme/projects/${UUID}/repository/bind/`;
const IDENTIFIER_PATH = "/api/workspaces/acme/project-identifiers/";
const ASSET_PATH = `/api/assets/v2/workspaces/acme/projects/${UUID}/`;
const USER_ASSET_PATH = "/api/assets/v2/user-assets/";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: UUID,
    created_at: CREATED_AT,
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    name: "Website relaunch",
    description: "Public site work",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: true,
    cycle_view: false,
    issue_views_view: true,
    page_view: false,
    intake_view: true,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: { in_use: "emoji", emoji: { value: "🌱" } },
    default_state: null,
    archived_at: null,
    timezone: "Europe/Lisbon",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "develop",
    is_favorite: false,
    sort_order: null,
    member_role: null,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "cloud_agent", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "not_configured" },
    ],
    default_agent_executor: "local_runner",
    agent_ticking_enabled: true,
    agent_default_max_ticks: 7,
    agent_default_interval_seconds: 21_600,
    agent_review_default_interval_seconds: 21_600,
    agent_test_default_interval_seconds: 21_600,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

interface StubOptions {
  row?: (createdAt: string) => Record<string, unknown>;
  hangDetail?: boolean;
  detailStatus?: number;
  identifierExists?: number;
  patchStatus?: number;
  patchBody?: unknown;
  patchRows?: Array<Record<string, unknown>>;
  patchNeverAnswers?: boolean;
  bindStatus?: number;
  bound?: Record<string, unknown>;
  uploadOk?: boolean;
  /** Number of leading upload grants that fail before one succeeds. */
  uploadFailures?: number;
}

function stubApi(options: StubOptions = {}) {
  const calls: Call[] = [];
  const patchRows = [...(options.patchRows ?? [])];
  let bound = options.bound ?? { bound: false };
  let uploadFailures = options.uploadFailures ?? 0;
  let currentRow = options.row?.(CREATED_AT) ?? projectRow();
  // The asset byte leg posts to a signed URL outside the app transport.
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({ ok: true, status: 204 }))
  );
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${PROJECT_PATH}` && method === "GET") {
      if (options.hangDetail === true) return new Promise<never>(() => undefined);
      const status = options.detailStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, { detail: "gone" });
      return jsonResponse(url, 200, currentRow);
    }
    if (url === `https://api.test${PROJECT_PATH}` && method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) return jsonResponse(url, status, options.patchBody ?? { detail: "refused" });
      if (options.patchNeverAnswers === true) return new Promise<never>(() => undefined);
      currentRow =
        patchRows.shift() ??
        projectRow({
          ...(body as Record<string, unknown>),
          description_text: null,
          description_html: null,
          updated_at: "2026-02-04T00:00:00Z",
        });
      return jsonResponse(url, 200, currentRow);
    }
    if (url.startsWith(`https://api.test${REPOSITORY_PATH}`) && method === "GET") {
      return jsonResponse(url, 200, bound);
    }
    if (url === `https://api.test${BIND_PATH}` && method === "POST") {
      const status = options.bindStatus ?? 201;
      if (status !== 201) return jsonResponse(url, status, { detail: "no provider account" });
      bound = {
        bound: true,
        id: "323e4567-e89b-12d3-a456-426614174002",
        provider: "github",
        provider_account_id: null,
        host_url: "https://github.com",
        repository: {
          id: "repo-1",
          provider: "github",
          host_url: "https://github.com",
          namespace: "acme",
          name: "site",
          full_name: "acme/site",
          web_url: "https://github.com/acme/site",
          clone_url_http: "https://github.com/acme/site.git",
          clone_url_ssh: "git@github.com:acme/site.git",
          default_branch: "main",
          private: true,
        },
        is_sync_enabled: false,
        clone_auth_mode: "runner_managed",
        last_synced_at: null,
        last_sync_error: "",
        degraded: false,
        degraded_reason: null,
        repo_url: "https://github.com/acme/site",
      };
      return jsonResponse(url, 201, bound);
    }
    if (url.startsWith(`https://api.test${IDENTIFIER_PATH}`) && method === "GET")
      return reserved(options.identifierExists ?? 0);
    if (url === "https://api.test/api/timezones/") {
      return jsonResponse(url, 200, {
        timezones: [{ value: "Europe/Lisbon", label: "Lisbon", gmt_offset: "GMT+0", utc_offset: "+00:00" }],
      });
    }
    if (url === `https://api.test${ASSET_PATH}` && method === "POST") {
      if (options.uploadOk === false) return jsonResponse(url, 500, { detail: "no" });
      if (uploadFailures > 0) {
        uploadFailures -= 1;
        return jsonResponse(url, 500, { detail: "no" });
      }
      return jsonResponse(url, 200, {
        asset_id: "asset-1",
        asset_url: "/api/assets/v2/static/emblem/",
        upload_data: { url: "https://uploads.test/signed", fields: { key: "emblems/asset-1.png" } },
      });
    }
    if (url === `https://api.test${ASSET_PATH}asset-1/` && method === "PATCH") {
      return jsonResponse(url, 204, {});
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls };
}

function reserved(exists: number) {
  return jsonResponse(`https://api.test${IDENTIFIER_PATH}`, 200, {
    exists,
    identifiers: exists === 0 ? [] : [{ id: 1, name: "NEW", project: "323e4567-e89b-12d3-a456-426614174002" }],
  });
}

function renderTab(queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })): QueryClient {
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectSettingsGeneralTab workspaceSlug="acme" projectId={UUID} />
    </QueryClientProvider>
  );
  return queryClient;
}

async function renderLoaded(options: StubOptions = {}): Promise<{ calls: Call[]; queryClient: QueryClient }> {
  const { calls } = stubApi(options);
  const queryClient = renderTab();
  await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
  await waitFor(() => expect(queryClient.getQueryData(generalKeys.repository("acme", UUID))).toBeDefined());
  return { calls, queryClient };
}

function patchCalls(calls: Call[]): Call[] {
  return calls.filter((call) => call.method === "PATCH" && call.url === `https://api.test${PROJECT_PATH}`);
}

function field(id: string): HTMLInputElement {
  return screen.getByRole("textbox", { name: id }) as HTMLInputElement;
}

function save(): void {
  fireEvent.click(screen.getByRole("button", { name: "Update project" }));
}

function patchBodyAt(calls: Call[], index = 0): Record<string, unknown> {
  return (patchCalls(calls)[index]?.body ?? {}) as Record<string, unknown>;
}

afterEach(() => {
  cleanup();
  resetAppClient();
  harness.role = ROLE_ADMIN;
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("PSET-005 read view", () => {
  it("draws a shaped skeleton until the row arrives, with no cover banner", async () => {
    stubApi({ hangDetail: true });
    renderTab();
    expect(screen.getByTestId("project-settings-general-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("project-settings-general")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /cover/i })).not.toBeInTheDocument();
  });

  it("renders every stored field", async () => {
    await renderLoaded();
    expect(screen.getByTestId("general-headline")).toHaveTextContent("Website relaunch");
    expect(screen.getByTestId("general-headline-identifier")).toHaveTextContent("WEB");
    expect(screen.getByTestId("general-network")).toHaveTextContent("Public");
    expect(screen.getByTestId("general-created")).toHaveTextContent(/^Created on /);
    expect(field("Project name")).toHaveValue("Website relaunch");
    expect(field("Short code")).toHaveValue("WEB");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Public site work");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveAttribute(
      "placeholder",
      DESCRIPTION_PLACEHOLDER
    );
    expect(field("Base branch")).toHaveValue("develop");
    expect(field("Repository URL")).toHaveValue("");
    expect(screen.getByTestId("general-visibility")).toHaveTextContent("Public");
    expect(screen.getByTestId("general-timezone")).toHaveTextContent("Europe/Lisbon");
    expect(screen.getByTestId("general-executor")).toHaveTextContent("Local runner");
    expect(screen.getByRole("spinbutton", { name: "Run budget per work item" })).toHaveValue(7);
    expect(screen.getByTestId("general-cadence")).toHaveTextContent("6 hours");
    expect(screen.queryByTestId("general-banner")?.className).not.toMatch(/cover/);
  });

  it("marks the custom cadence when the stored rhythm is out of band", async () => {
    await renderLoaded({ row: () => projectRow({ agent_default_interval_seconds: 5_400 }) });
    expect(screen.getByTestId("general-cadence")).toHaveTextContent("Custom");
  });

  it("draws a blank row as untitled with no stamp", async () => {
    await renderLoaded({ row: () => projectRow({ name: "", identifier: "" }) });
    expect(screen.getByTestId("general-headline")).toHaveTextContent("Untitled project");
    expect(screen.getByTestId("general-headline-identifier")).toHaveTextContent("No short code");
  });
});

describe("PSET-006 save and validation", () => {
  it("saves an admin's rename and confirms", async () => {
    const { calls, queryClient } = await renderLoaded();
    fireEvent.change(field("Project name"), { target: { value: "Website relaunch v2" } });
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls)).toMatchObject({ name: "Website relaunch v2", identifier: "WEB" });
    expect(notify.success).toHaveBeenCalledWith(PROJECT_SAVED_TITLE, PROJECT_SAVED_MESSAGE);
    expect(notify.error).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(queryClient.getQueryData(generalKeys.detail("acme", UUID))).toMatchObject({ name: "Website relaunch v2" })
    );
  });

  it("blocks a blank name and identifier without a request", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Project name"), { target: { value: "   " } });
    fireEvent.change(field("Short code"), { target: { value: "  " } });
    save();
    expect(await screen.findByText(NAME_REQUIRED_MESSAGE)).toBeInTheDocument();
    expect(screen.getByText("A short code is required")).toBeInTheDocument();
    expect(patchCalls(calls)).toHaveLength(0);
  });

  it("shapes the short code live to uppercase alphanumerics", async () => {
    await renderLoaded();
    fireEvent.change(field("Short code"), { target: { value: "ab-12_cd" } });
    expect(field("Short code")).toHaveValue("AB12CD");
  });

  it("rejects a base branch outside the character set", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Base branch"), { target: { value: "feature branch" } });
    save();
    expect(await screen.findByText(BASE_BRANCH_CHARS_MESSAGE)).toBeInTheDocument();
    expect(patchCalls(calls)).toHaveLength(0);
  });

  it("maps a write-time identifier collision onto its field", async () => {
    const { calls } = await renderLoaded({
      patchStatus: 400,
      patchBody: { identifier: ["PROJECT_IDENTIFIER_ALREADY_EXIST"] },
    });
    fireEvent.change(field("Short code"), { target: { value: "NEW" } });
    await waitFor(() => expect(calls.some((call) => call.url.includes("project-identifiers/?name=NEW"))).toBe(true));
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(await screen.findByText(IDENTIFIER_TAKEN_MESSAGE)).toBeInTheDocument();
    expect(notify.error).not.toHaveBeenCalled();
  });

  it("blocks a reserved code from the availability precheck", async () => {
    const { calls } = await renderLoaded({ identifierExists: 1 });
    fireEvent.change(field("Short code"), { target: { value: "new" } });
    await waitFor(() => expect(calls.some((call) => call.url.includes("project-identifiers/?name=NEW"))).toBe(true));
    await waitFor(() => expect(screen.getByText("Up to 10 letters and numbers")).toBeInTheDocument());
    save();
    expect(await screen.findByText(IDENTIFIER_UNAVAILABLE_MESSAGE)).toBeInTheDocument();
    expect(patchCalls(calls)).toHaveLength(0);
  });

  it("saves a changed code the precheck reports free", async () => {
    const { calls } = await renderLoaded({ identifierExists: 0 });
    fireEvent.change(field("Short code"), { target: { value: "new" } });
    await waitFor(() => expect(calls.some((call) => call.url.includes("project-identifiers/?name=NEW"))).toBe(true));
    await waitFor(() => expect(screen.getByText("Up to 10 letters and numbers")).toBeInTheDocument());
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).identifier).toBe("NEW");
  });

  it("skips the precheck for a code that case-folds to the stored one", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Short code"), { target: { value: "web" } });
    expect(field("Short code")).toHaveValue("WEB");
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(calls.some((call) => call.url.includes("project-identifiers/"))).toBe(false);
  });

  it("clears a field error as soon as that field is edited", async () => {
    await renderLoaded();
    fireEvent.change(field("Project name"), { target: { value: "   " } });
    save();
    expect(await screen.findByText(NAME_REQUIRED_MESSAGE)).toBeInTheDocument();
    fireEvent.change(field("Project name"), { target: { value: "Website relaunch" } });
    expect(screen.queryByText(NAME_REQUIRED_MESSAGE)).not.toBeInTheDocument();
  });

  it("keeps the draft while a save is in flight", async () => {
    stubApi({ patchNeverAnswers: true });
    renderTab();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    fireEvent.change(field("Project name"), { target: { value: "In flight" } });
    save();
    expect(await screen.findByRole("button", { name: "Updating" })).toBeInTheDocument();
    expect(field("Project name")).toHaveValue("In flight");
  });

  it("ignores a second submit while the first save is in flight", async () => {
    const { calls } = stubApi({ patchNeverAnswers: true });
    renderTab();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    save();
    await screen.findByRole("button", { name: "Updating" });
    fireEvent.submit(screen.getByTestId("project-settings-general"));
    expect(patchCalls(calls)).toHaveLength(1);
  });

  it("maps a write-time name collision onto its field", async () => {
    stubApi({ patchStatus: 400, patchBody: { name: ["PROJECT_NAME_ALREADY_EXIST"] } });
    renderTab();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    fireEvent.change(field("Project name"), { target: { value: "Taken" } });
    save();
    expect(await screen.findByText("The project name is already taken.")).toBeInTheDocument();
  });

  it("keeps the form open and notices a generic failure", async () => {
    stubApi({ patchStatus: 500 });
    renderTab();
    await waitFor(() => expect(screen.getByTestId("project-settings-general")).toBeInTheDocument());
    save();
    await waitFor(() => expect(notify.error).toHaveBeenCalledWith("Error!", expect.stringContaining("refused")));
    expect(screen.getByTestId("project-settings-general")).toBeInTheDocument();
    expect(notify.success).not.toHaveBeenCalled();
  });

  it("dims and disables every control for a non-admin, with no save or danger zone", async () => {
    harness.role = ROLE_GUEST;
    await renderLoaded();
    expect(screen.getByTestId("project-settings-general").className).toMatch(/opacity-60/);
    expect(field("Project name")).toBeDisabled();
    expect(field("Short code")).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Description" })).toBeDisabled();
    expect(field("Base branch")).toBeDisabled();
    expect(field("Repository URL")).toBeDisabled();
    expect(screen.getByTestId("general-visibility-trigger")).toBeDisabled();
    expect(screen.getByTestId("general-executor-trigger")).toBeDisabled();
    expect(screen.getByTestId("general-ticking")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("general-repo-bind")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Update project" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("general-emblem-open")).not.toBeInTheDocument();
    expect(screen.queryByText(/archive|delete project/i)).not.toBeInTheDocument();
  });

  it.each([ROLE_MEMBER, ROLE_GUEST])("leaves role %i read-only", async (role) => {
    harness.role = role;
    await renderLoaded();
    expect(field("Project name")).toBeDisabled();
  });
});

describe("PSET-007 emblem", () => {
  it("picks an emoji mark and saves it as the stored value record", async () => {
    const user = userEvent.setup();
    const { calls } = await renderLoaded();
    await user.click(screen.getByTestId("general-emblem-open"));
    const picker = await screen.findByRole("dialog", { name: "Change emblem" });
    await user.click(within(picker).getByRole("button", { name: "🔥" }));
    // The draft header renders the new mark before the save lands.
    expect(screen.getByTestId("project-mark")).toHaveAttribute("data-mark", "emoji");
    expect(screen.getByTestId("project-mark")).toHaveTextContent("🔥");
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).logo_props).toEqual({ in_use: "emoji", emoji: { value: "128293" } });
  });

  it("filters the emoji grid and reports no match", async () => {
    const user = userEvent.setup();
    await renderLoaded();
    await user.click(screen.getByTestId("general-emblem-open"));
    const picker = await screen.findByRole("dialog", { name: "Change emblem" });
    await user.type(within(picker).getByRole("textbox", { name: "Search emoji" }), "🔥");
    expect(within(picker).getAllByTestId("general-emblem-choice")).toHaveLength(1);
    await user.type(within(picker).getByRole("textbox", { name: "Search emoji" }), "x");
    expect(within(picker).getByText("No emoji match that search.")).toBeInTheDocument();
  });

  it("uploads an image as a project asset and marks the draft", async () => {
    const { calls } = await renderLoaded();
    await userEvent.setup().click(screen.getByTestId("general-emblem-upload-open"));
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });
    fireEvent.change(screen.getByTestId("general-emblem-file"), { target: { files: [file] } });
    expect(await screen.findByTestId("project-mark-image")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/emblem/"
    );
    expect(screen.getByTestId("project-mark")).toHaveAttribute("data-mark", "image");
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).logo_props).toEqual({ in_use: "upload", url: "/api/assets/v2/static/emblem/" });
    expect(calls.find((call) => call.url === `https://api.test${ASSET_PATH}`)?.body).toMatchObject({
      entity_type: "PROJECT_COVER",
      entity_identifier: UUID,
    });
    expect(calls.some((call) => call.url === `https://api.test${USER_ASSET_PATH}`)).toBe(false);
    expect(calls.some((call) => call.url === `https://api.test${ASSET_PATH}asset-1/` && call.method === "PATCH")).toBe(
      true
    );
  });

  it("refuses a non-image file and uploads nothing", async () => {
    const { calls } = await renderLoaded();
    fireEvent.click(screen.getByTestId("general-emblem-upload-open"));
    const file = new File(["<svg/>"], "emblem.svg", { type: "image/svg+xml" });
    fireEvent.change(screen.getByTestId("general-emblem-file"), { target: { files: [file] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Choose a PNG, JPEG, WebP or GIF image.");
    expect(calls.some((call) => call.url === `https://api.test${ASSET_PATH}`)).toBe(false);
  });

  it("aborts the whole save when the upload fails, even with a valid form", async () => {
    const { calls } = await renderLoaded({ uploadOk: false });
    fireEvent.change(field("Project name"), { target: { value: "Renamed despite the failure" } });
    await userEvent.setup().click(screen.getByTestId("general-emblem-upload-open"));
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });
    fireEvent.change(screen.getByTestId("general-emblem-file"), { target: { files: [file] } });
    await waitFor(() => expect(notify.error).toHaveBeenCalled());
    await userEvent.setup().keyboard("{Escape}");
    save();
    expect(patchCalls(calls)).toHaveLength(0);
    expect(screen.getByTestId("project-settings-general")).toBeInTheDocument();
  });

  it("lets a later successful upload clear the block", async () => {
    const { calls } = await renderLoaded({ uploadFailures: 1 });
    fireEvent.change(field("Project name"), { target: { value: "Renamed" } });
    await userEvent.setup().click(screen.getByTestId("general-emblem-upload-open"));
    fireEvent.change(screen.getByTestId("general-emblem-file"), {
      target: { files: [new File(["bytes"], "emblem.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(notify.error).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("general-emblem-file"), {
      target: { files: [new File(["bytes"], "emblem.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByTestId("project-mark")).toHaveAttribute("data-mark", "image"));
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls)).toMatchObject({ name: "Renamed", logo_props: { in_use: "upload" } });
  });

  it("seeds the shell's project row so the header and rail follow a new emblem", async () => {
    const user = userEvent.setup();
    const { queryClient } = await renderLoaded();
    await user.click(screen.getByTestId("general-emblem-open"));
    const picker = await screen.findByRole("dialog", { name: "Change emblem" });
    await user.click(within(picker).getByRole("button", { name: "🔥" }));
    save();
    await waitFor(() =>
      expect(queryClient.getQueryData(authKeys.project("acme", UUID))).toMatchObject({
        logo_props: { in_use: "emoji", emoji: { value: "128293" } },
      })
    );
  });

  it("offers no cover control, gallery or provider search", async () => {
    const user = userEvent.setup();
    await renderLoaded();
    expect(screen.queryByTestId("general-cover")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /cover/i })).not.toBeInTheDocument();
    await user.click(screen.getByTestId("general-emblem-open"));
    const picker = await screen.findByRole("dialog", { name: "Change emblem" });
    expect(within(picker).queryByText(/gallery|unsplash|search images/i)).not.toBeInTheDocument();
    expect(within(picker).queryByRole("link")).not.toBeInTheDocument();
  });
});

describe("PSET-008 repository bind", () => {
  it("keeps the URL out of the normal save payload", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Repository URL"), { target: { value: "https://github.com/acme/site" } });
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls)["repo_url"]).toBeUndefined();
  });

  it("refuses an empty bind with a notice and no request", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Repository URL"), { target: { value: "   " } });
    fireEvent.click(screen.getByTestId("general-repo-bind"));
    expect(notify.error).toHaveBeenCalledWith(REPO_EMPTY_TITLE, REPO_EMPTY_MESSAGE);
    expect(calls.some((call) => call.url === `https://api.test${BIND_PATH}`)).toBe(false);
  });

  it("binds, writes the canonical URL back and reads as bound", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Repository URL"), { target: { value: " github.com/acme/site " } });
    fireEvent.click(screen.getByTestId("general-repo-bind"));
    await waitFor(() => expect(screen.getByTestId("general-repo-bound")).toBeInTheDocument());
    expect(calls.find((call) => call.url === `https://api.test${BIND_PATH}`)?.body).toEqual({
      repo_url: "github.com/acme/site",
    });
    expect(patchBodyAt(calls)).toEqual({ repo_url: "https://github.com/acme/site" });
    expect(field("Repository URL")).toHaveValue("https://github.com/acme/site");
    expect(notify.success).toHaveBeenCalledWith(REPO_BOUND_TITLE, expect.any(String));
  });

  it("shows the bound pill only on an exact canonical match", async () => {
    await renderLoaded({
      bound: {
        bound: true,
        id: "323e4567-e89b-12d3-a456-426614174002",
        provider: "github",
        provider_account_id: null,
        host_url: "https://github.com",
        repository: {
          id: "repo-1",
          provider: "github",
          host_url: "https://github.com",
          namespace: "acme",
          name: "site",
          full_name: "acme/site",
          web_url: "https://github.com/acme/site",
          clone_url_http: "https://github.com/acme/site.git",
          clone_url_ssh: "git@github.com:acme/site.git",
          default_branch: "main",
          private: true,
        },
        is_sync_enabled: false,
        clone_auth_mode: "runner_managed",
        last_synced_at: null,
        last_sync_error: "",
        degraded: false,
        degraded_reason: null,
        repo_url: "https://github.com/acme/site",
      },
      row: () => projectRow({ repo_url: "https://github.com/acme/site" }),
    });
    fireEvent.change(field("Repository URL"), { target: { value: "https://github.com/acme/other" } });
    expect(screen.getByTestId("general-repo-bind")).toBeInTheDocument();
    fireEvent.change(field("Repository URL"), { target: { value: "https://github.com/acme/site" } });
    expect(screen.getByTestId("general-repo-bound")).toBeDisabled();
  });

  it("surfaces a bind refusal and keeps the bind control", async () => {
    const { calls } = await renderLoaded({ bindStatus: 409 });
    fireEvent.change(field("Repository URL"), { target: { value: "https://github.com/acme/site" } });
    fireEvent.click(screen.getByTestId("general-repo-bind"));
    await waitFor(() => expect(notify.error).toHaveBeenCalledWith(REPO_BIND_FAILED_TITLE, expect.any(String)));
    expect(patchCalls(calls)).toHaveLength(0);
    expect(screen.getByTestId("general-repo-bind")).toBeEnabled();
  });

  it("caps the repository URL input at 512 characters", async () => {
    await renderLoaded();
    expect(field("Repository URL")).toHaveAttribute("maxLength", "512");
  });
});

describe("PSET-009 execution policy", () => {
  it("disables an unavailable family but never the local runner", async () => {
    const user = userEvent.setup();
    await renderLoaded();
    await user.click(screen.getByTestId("general-executor-trigger"));
    expect(screen.getByRole("option", { name: "Local runner" })).toBeEnabled();
    expect(screen.getByRole("option", { name: "Cloud agent" })).toBeEnabled();
    expect(screen.getByRole("option", { name: "Desktop agent" })).toBeDisabled();
  });

  it("saves a switched execution target", async () => {
    const user = userEvent.setup();
    const { calls } = await renderLoaded();
    await user.click(screen.getByTestId("general-executor-trigger"));
    await user.click(screen.getByRole("option", { name: "Cloud agent" }));
    expect(screen.getByTestId("general-executor")).toHaveTextContent("Cloud agent");
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).default_agent_executor).toBe("cloud_agent");
  });

  it("leaves the policy visible but inert while automated runs are off", async () => {
    await renderLoaded();
    fireEvent.click(screen.getByTestId("general-ticking"));
    expect(screen.getByTestId("general-ticking")).toHaveAttribute("aria-checked", "false");
    const budget = screen.getByRole("spinbutton", { name: "Run budget per work item" });
    expect(budget).toBeDisabled();
    expect(screen.getByTestId("general-cadence-trigger")).toBeDisabled();
    expect(screen.getByTestId("general-nocap")).toBeDisabled();
    expect(budget).toHaveValue(7);
  });

  it("refuses a cleared budget while ticking, with a field error", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(screen.getByRole("spinbutton", { name: "Run budget per work item" }), { target: { value: "" } });
    save();
    expect(await screen.findByText(BUDGET_REQUIRED_MESSAGE)).toBeInTheDocument();
    expect(patchCalls(calls)).toHaveLength(0);
  });

  it("remembers the capped value across a no-cap tick", async () => {
    const budget = () => screen.getByRole("spinbutton", { name: "Run budget per work item" }) as HTMLInputElement;
    const { calls } = await renderLoaded();
    fireEvent.change(budget(), { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("general-nocap"));
    expect(budget()).toHaveValue(null);
    fireEvent.click(screen.getByTestId("general-nocap"));
    expect(budget()).toHaveValue(12);
    fireEvent.change(budget(), { target: { value: "3" } });
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).agent_default_max_ticks).toBe(3);
  });

  it("writes the no-cap sentinel", async () => {
    const { calls } = await renderLoaded();
    fireEvent.click(screen.getByTestId("general-nocap"));
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls).agent_default_max_ticks).toBe(-1);
  });

  it("writes a picked cadence to every stage", async () => {
    const user = userEvent.setup();
    const { calls } = await renderLoaded();
    await user.click(screen.getByTestId("general-cadence-trigger"));
    await user.click(screen.getByRole("option", { name: "6 hours" }));
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls)).toMatchObject({
      agent_default_interval_seconds: 21_600,
      agent_review_default_interval_seconds: 21_600,
      agent_test_default_interval_seconds: 21_600,
    });
  });

  it("leaves per-stage rhythms alone on an unrelated save", async () => {
    const { calls } = await renderLoaded();
    fireEvent.change(field("Project name"), { target: { value: "Renamed" } });
    save();
    await waitFor(() => expect(patchCalls(calls)).toHaveLength(1));
    expect(patchBodyAt(calls)["agent_default_interval_seconds"]).toBeUndefined();
    expect(patchBodyAt(calls)["agent_review_default_interval_seconds"]).toBeUndefined();
    expect(patchBodyAt(calls)["agent_test_default_interval_seconds"]).toBeUndefined();
  });
});
