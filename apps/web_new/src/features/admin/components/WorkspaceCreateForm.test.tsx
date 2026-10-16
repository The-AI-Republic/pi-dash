// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute as createTestRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { resetAdminStores, useWorkspaceCreateDraftStore } from "../stores.js";
import {
  WORKSPACE_CREATE_FAILURE_TITLE,
  WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_TITLE,
} from "../workspaceCreate.js";
import { WorkspaceCreateForm, WORKSPACE_CREATE_SIZE_PLACEHOLDER } from "./WorkspaceCreateForm.js";

interface SeenRequest {
  method: string;
  url: string;
  body: unknown;
}

const createdWorkspace = {
  id: "33333333-3333-4333-8333-333333333333",
  name: "Good Name",
  slug: "good-slug-1",
  logo: null,
  logo_asset: null,
  logo_url: null,
  owner: {
    id: "22222222-2222-4222-8222-222222222222",
    email: "admin@example.com",
    first_name: "Admin",
    last_name: "User",
  },
  organization_size: "2-10",
  timezone: "UTC",
  background_color: "#ffffff",
  created_by: null,
  updated_by: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function recordingTransport(
  seen: SeenRequest[],
  routes: Record<
    string,
    { status: number; body: unknown } | ((url: string) => Promise<{ status: number; body: unknown }>)
  >
): Transport {
  return (url, init) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    const body = init?.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
    seen.push({ method: init?.method ?? "GET", url, body });
    const answer = typeof route === "function" ? route(url) : Promise.resolve(route);
    return answer.then((resolved) => ({
      status: resolved.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(resolved.body)),
    }));
  };
}

async function renderForm(transport: Transport): Promise<{ pathname: () => string; unmount: () => void }> {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const createRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/workspace/create",
    component: () => <WorkspaceCreateForm />,
  });
  const listRoute = createTestRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/workspace",
    component: () => <div data-testid="workspace-list-stub">list</div>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([createRoute, listRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/workspace/create/"] }),
    trailingSlash: "preserve",
  });
  const { unmount } = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByTestId("admin-workspace-create-form");
  return { pathname: () => router.state.location.pathname, unmount };
}

function fillValidForm(): void {
  fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Good Name" } });
  fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "good-slug-1" } });
  fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "2-10" } });
}

beforeEach(() => {
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("WorkspaceCreateForm", () => {
  it("renders the fields prefixed, unpicked, and submit-gated with no messages", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    expect(screen.getByTestId("admin-workspace-create-slug-prefix")).toHaveTextContent(`${window.location.origin}/`);
    expect(screen.getByLabelText("Workspace name")).toBe(screen.getByTestId("admin-workspace-create-name"));
    expect(screen.getByLabelText("Workspace URL")).toBe(screen.getByTestId("admin-workspace-create-slug"));
    expect(screen.getByLabelText("Organization size")).toBe(screen.getByTestId("admin-workspace-create-org-size"));
    const size = screen.getByTestId("admin-workspace-create-org-size") as HTMLSelectElement;
    expect(size.value).toBe("");
    expect(size.options[0]?.text).toBe(WORKSPACE_CREATE_SIZE_PLACEHOLDER);
    expect(Array.from(size.options).map((option) => option.value)).toEqual([
      "",
      "Just myself",
      "2-10",
      "11-50",
      "51-200",
      "201-500",
      "500+",
    ]);
    const submit = screen.getByTestId("admin-workspace-create-submit") as HTMLButtonElement;
    expect(submit).toBeDisabled();
    expect(submit).toHaveTextContent("Create workspace");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("derives the slug from the name while keeping it independently editable and lowercase", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    const name = screen.getByTestId("admin-workspace-create-name") as HTMLInputElement;
    const slug = screen.getByTestId("admin-workspace-create-slug") as HTMLInputElement;
    fireEvent.change(name, { target: { value: "My Cool Workspace" } });
    expect(slug.value).toBe("my-cool-workspace");
    fireEvent.change(slug, { target: { value: "custom-slug-1" } });
    expect(slug.value).toBe("custom-slug-1");
    expect(name.value).toBe("My Cool Workspace");
    fireEvent.change(slug, { target: { value: "UPPER-SLUG" } });
    expect(slug.value).toBe("upper-slug");
  });

  it("opens submit once the name, slug, and size band validate", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    const submit = screen.getByTestId("admin-workspace-create-submit") as HTMLButtonElement;
    expect(submit).toBeDisabled();
    fillValidForm();
    expect(submit).not.toBeDisabled();
    expect(submit).toHaveTextContent("Create workspace");
  });

  it("reports the name failure inline and clears it on a good name", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "2-10" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Bad{Name}" } });
    expect(screen.getByRole("alert")).toHaveTextContent("Workspace name");
    expect(screen.getByTestId("admin-workspace-create-submit")).toBeDisabled();
    fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Good Name" } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports the slug charset hint inline and clears it on a good slug", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    fillValidForm();
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "bad slug!" } });
    expect(screen.getByTestId("admin-workspace-create-slug-invalid")).toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-create-submit")).toBeDisabled();
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "good-slug-1" } });
    expect(screen.queryByTestId("admin-workspace-create-slug-invalid")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-workspace-create-submit")).not.toBeDisabled();
  });

  it("reports the size as required when the placeholder is picked", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "2-10" } });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "" } });
    expect(screen.getByRole("alert")).toHaveTextContent("Choose an organization size.");
  });

  it("creates nothing when an invalid form is submitted", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(recordingTransport(seen, {}));
    fireEvent.submit(screen.getByTestId("admin-workspace-create-form"));
    expect(seen).toHaveLength(0);
  });

  it("shows the inline taken error with no create when the probe reports unavailable", async () => {
    const seen: SeenRequest[] = [];
    const { pathname } = await renderForm(
      recordingTransport(seen, { "/api/instances/workspace-slug-check/": { status: 200, body: { status: false } } })
    );
    fillValidForm();
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() => expect(screen.getByTestId("admin-workspace-create-slug-taken")).toBeInTheDocument());
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toContain("slug=good-slug-1");
    expect(notifyError).not.toHaveBeenCalled();
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(pathname()).toContain("/workspace/create");
  });

  it("shows the taken error with no create for a restricted slug on a clear probe", async () => {
    const seen: SeenRequest[] = [];
    await renderForm(
      recordingTransport(seen, { "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } } })
    );
    fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Restricted Probe" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "god-mode" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "2-10" } });
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() => expect(screen.getByTestId("admin-workspace-create-slug-taken")).toBeInTheDocument());
    expect(seen).toHaveLength(1);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts the probe failure and stays with the values intact", async () => {
    const seen: SeenRequest[] = [];
    const { pathname } = await renderForm(
      recordingTransport(seen, { "/api/instances/workspace-slug-check/": { status: 500, body: { detail: "boom" } } })
    );
    fillValidForm();
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(WORKSPACE_CREATE_FAILURE_TITLE, WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE)
    );
    expect(seen).toHaveLength(1);
    expect(screen.queryByTestId("admin-workspace-create-slug-taken")).not.toBeInTheDocument();
    expect(pathname()).toContain("/workspace/create");
    expect((screen.getByTestId("admin-workspace-create-name") as HTMLInputElement).value).toBe("Good Name");
    expect((screen.getByTestId("admin-workspace-create-slug") as HTMLInputElement).value).toBe("good-slug-1");
  });

  it("toasts the create failure and stays with the values intact", async () => {
    const seen: SeenRequest[] = [];
    const { pathname } = await renderForm(
      recordingTransport(seen, {
        "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } },
        "/api/instances/workspaces/": { status: 500, body: { detail: "boom" } },
      })
    );
    fillValidForm();
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(WORKSPACE_CREATE_FAILURE_TITLE, WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE)
    );
    expect(seen.filter((request) => request.url.includes("/api/instances/"))).toHaveLength(2);
    expect(seen.filter((request) => request.method === "POST")).toHaveLength(1);
    expect(pathname()).toContain("/workspace/create");
    expect((screen.getByTestId("admin-workspace-create-name") as HTMLInputElement).value).toBe("Good Name");
  });

  it("creates with no owner, toasts success, clears the draft, and lands on the list", async () => {
    const seen: SeenRequest[] = [];
    const { pathname } = await renderForm(
      recordingTransport(seen, {
        "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } },
        "/api/instances/workspaces/": { status: 201, body: createdWorkspace },
      })
    );
    fillValidForm();
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE)
    );
    const posts = seen.filter((request) => request.method === "POST");
    expect(posts).toHaveLength(1);
    expect(posts[0]?.url).toContain("/api/instances/workspaces/");
    expect(posts[0]?.body).toEqual({ name: "Good Name", slug: "good-slug-1", organization_size: "2-10" });
    expect(posts[0]?.body).not.toHaveProperty("owner");
    expect(useWorkspaceCreateDraftStore.getState()).toMatchObject({ name: "", slug: "", organizationSize: "" });
    await waitFor(() => expect(pathname()).toBe("/god-mode/workspace"));
    expect(await screen.findByTestId("workspace-list-stub")).toBeInTheDocument();
  });

  it("ignores a second submit while the create is in flight", async () => {
    const seen: SeenRequest[] = [];
    let releaseCreate!: (answer: { status: number; body: unknown }) => void;
    const gate = new Promise<{ status: number; body: unknown }>((resolve) => {
      releaseCreate = resolve;
    });
    await renderForm(
      recordingTransport(seen, {
        "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } },
        "/api/instances/workspaces/": () => gate,
      })
    );
    fillValidForm();
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() =>
      expect(screen.getByTestId("admin-workspace-create-submit")).toHaveTextContent("Creating workspace")
    );
    fireEvent.submit(screen.getByTestId("admin-workspace-create-form"));
    releaseCreate({ status: 201, body: createdWorkspace });
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE)
    );
    expect(seen.filter((request) => request.method === "POST")).toHaveLength(1);
  });

  it("shows the busy label while the create is in flight", async () => {
    const seen: SeenRequest[] = [];
    let releaseCreate!: (answer: { status: number; body: unknown }) => void;
    const gate = new Promise<{ status: number; body: unknown }>((resolve) => {
      releaseCreate = resolve;
    });
    await renderForm(
      recordingTransport(seen, {
        "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } },
        "/api/instances/workspaces/": () => gate,
      })
    );
    fillValidForm();
    const submit = screen.getByTestId("admin-workspace-create-submit") as HTMLButtonElement;
    fireEvent.click(submit);
    await waitFor(() => expect(submit).toHaveTextContent("Creating workspace"));
    expect(submit).toBeDisabled();
    expect(submit).toHaveAttribute("aria-busy", "true");
    releaseCreate({ status: 201, body: createdWorkspace });
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE)
    );
  });

  it("clears a prior taken error once a later submit succeeds", async () => {
    const seen: SeenRequest[] = [];
    const { pathname } = await renderForm(
      recordingTransport(seen, {
        "/api/instances/workspace-slug-check/": (url) => {
          const slug = new URL(url, "https://api.test").searchParams.get("slug");
          return Promise.resolve({ status: 200, body: { status: slug !== "taken-slug" } });
        },
        "/api/instances/workspaces/": { status: 201, body: createdWorkspace },
      })
    );
    fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Taken Probe" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "taken-slug" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "2-10" } });
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() => expect(screen.getByTestId("admin-workspace-create-slug-taken")).toBeInTheDocument());
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "free-slug" } });
    fireEvent.click(screen.getByTestId("admin-workspace-create-submit"));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE)
    );
    await waitFor(() => expect(pathname()).toBe("/god-mode/workspace"));
  });

  it("keeps typed values across unmount", async () => {
    const seen: SeenRequest[] = [];
    const first = await renderForm(recordingTransport(seen, {}));
    fireEvent.change(screen.getByTestId("admin-workspace-create-name"), { target: { value: "Half typed" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-slug"), { target: { value: "half-typed" } });
    fireEvent.change(screen.getByTestId("admin-workspace-create-org-size"), { target: { value: "11-50" } });
    first.unmount();
    resetAppClient();
    await renderForm(recordingTransport(seen, {}));
    expect((screen.getByTestId("admin-workspace-create-name") as HTMLInputElement).value).toBe("Half typed");
    expect((screen.getByTestId("admin-workspace-create-slug") as HTMLInputElement).value).toBe("half-typed");
    expect((screen.getByTestId("admin-workspace-create-org-size") as HTMLSelectElement).value).toBe("11-50");
  });
});
