// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-pod dialog (NEWFRONT-542, RUN-013). Both fields are required with
// their own messages; the project list loads only while the dialog is open
// and a failed read is explained inline; a submit names its working state
// and cannot be cancelled; success closes and failure keeps the dialog with
// a toast; reopening starts from a blank form.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ApiError, type Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks, notify } = vi.hoisted(() => ({
  contractMocks: { createPod: vi.fn(), listProjects: vi.fn() },
  notify: { notifyError: vi.fn(), notifySuccess: vi.fn(), notifyInfo: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, ...notify };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");

import {
  CREATE_POD_HINT,
  CREATE_POD_PROJECT_PLACEHOLDER,
  CREATE_POD_PROJECTS_FAILED,
  CREATE_POD_SUBMIT,
  CREATE_POD_SUBMITTING,
  CREATE_POD_TITLE,
  CreatePodDialog,
} from "./CreatePodDialog.js";
import { POD_CREATE_FAILED, POD_ERROR_TITLE } from "../pods.js";

function project(id: string, name: string): Project {
  return { id, name, identifier: id.toUpperCase() } as Project;
}

function setup(props: { open?: boolean; onClose?: () => void } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onClose = props.onClose ?? vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <CreatePodDialog workspaceSlug="acme" open={props.open ?? true} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose, queryClient, ...view };
}

async function chooseProject(name: string): Promise<void> {
  const trigger = screen.getByRole("button", { name: CREATE_POD_PROJECT_PLACEHOLDER });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
  fireEvent.click(await screen.findByRole("option", { name }));
}

beforeEach(() => {
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  for (const mock of Object.values(notify)) mock.mockReset();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  contractMocks.listProjects.mockResolvedValue([project("web", "Web app"), project("arc", "Arcade")]);
});

afterEach(() => resetAppClient());

describe("CreatePodDialog", () => {
  it("renders nothing while closed and reads no projects", () => {
    setup({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(contractMocks.listProjects).not.toHaveBeenCalled();
  });

  it("opens with its title, hint and closed project picker", async () => {
    setup();
    expect(await screen.findByRole("heading", { name: CREATE_POD_TITLE })).toBeInTheDocument();
    expect(screen.getByText(CREATE_POD_HINT)).toBeInTheDocument();
    const trigger = screen.getByRole("button", { name: CREATE_POD_PROJECT_PLACEHOLDER });
    await waitFor(() => expect(trigger).toBeEnabled());
    expect(contractMocks.listProjects).toHaveBeenCalledWith(expect.anything(), "acme");
    expect(screen.getByLabelText("Name")).toHaveValue("");
    expect(screen.getByLabelText("Description (optional)")).toHaveValue("");
  });

  it("names both required fields on an empty submit and sends nothing", async () => {
    const { onClose } = setup();
    await screen.findByRole("button", { name: CREATE_POD_SUBMIT });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    expect(await screen.findByText("Pick a project.")).toBeInTheDocument();
    expect(screen.getByText("Name is required.")).toBeInTheDocument();
    expect(contractMocks.createPod).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("clears each field's message once it is filled", async () => {
    setup();
    fireEvent.click(await screen.findByRole("button", { name: CREATE_POD_SUBMIT }));
    expect(await screen.findByText("Name is required.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    expect(screen.queryByText("Name is required.")).not.toBeInTheDocument();
    expect(screen.getByText("Pick a project.")).toBeInTheDocument();
    await chooseProject("Web app");
    expect(screen.queryByText("Pick a project.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Web app" })).toBeInTheDocument();
  });

  it("trims the values and sends the chosen project", async () => {
    const { onClose } = setup();
    contractMocks.createPod.mockResolvedValue({ id: "pod-1" });
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "  beefy  " } });
    fireEvent.change(screen.getByLabelText("Description (optional)"), { target: { value: "  the big boxes " } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    await waitFor(() =>
      expect(contractMocks.createPod).toHaveBeenCalledWith(expect.anything(), {
        project: "web",
        name: "beefy",
        description: "the big boxes",
      })
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("omits a blank description", async () => {
    setup();
    contractMocks.createPod.mockResolvedValue({ id: "pod-1" });
    await chooseProject("Arcade");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "pinball" } });
    fireEvent.change(screen.getByLabelText("Description (optional)"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    await waitFor(() =>
      expect(contractMocks.createPod).toHaveBeenCalledWith(expect.anything(), { project: "arc", name: "pinball" })
    );
  });

  it("blocks cancel and dismissal while the create is in flight", async () => {
    const { onClose } = setup();
    let resolve: ((value: unknown) => void) | undefined;
    contractMocks.createPod.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    const working = await screen.findByRole("button", { name: CREATE_POD_SUBMITTING });
    expect(working).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    resolve?.({ id: "pod-1" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("closes on Escape while no create is in flight", async () => {
    const { onClose } = setup();
    await screen.findByRole("heading", { name: CREATE_POD_TITLE });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contractMocks.createPod).not.toHaveBeenCalled();
  });

  it("keeps the dialog and toasts the server's reason on failure", async () => {
    const { onClose } = setup();
    contractMocks.createPod.mockRejectedValue(
      new ApiError({ status: 400, code: "http", message: "name already used" })
    );
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, "name already used"));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: CREATE_POD_TITLE })).toBeInTheDocument();
  });

  it("keeps the generic sentence when the request never reached the server", async () => {
    setup();
    contractMocks.createPod.mockRejectedValue(new ApiError({ status: 0, code: "network", message: "Failed to fetch" }));
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, POD_CREATE_FAILED));
  });

  it("toasts the generic sentence for a non-API failure", async () => {
    setup();
    contractMocks.createPod.mockRejectedValue(new Error(""));
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    fireEvent.click(screen.getByRole("button", { name: CREATE_POD_SUBMIT }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, POD_CREATE_FAILED));
  });

  it("resets the form whenever it is reopened", async () => {
    const onClose = vi.fn();
    const { rerender } = setup({ onClose });
    await chooseProject("Web app");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "beefy" } });
    fireEvent.change(screen.getByLabelText("Description (optional)"), { target: { value: "kept?" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <CreatePodDialog workspaceSlug="acme" open={false} onClose={onClose} />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <CreatePodDialog workspaceSlug="acme" open onClose={onClose} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("button", { name: CREATE_POD_PROJECT_PLACEHOLDER })).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("");
    expect(screen.getByLabelText("Description (optional)")).toHaveValue("");
  });

  it("explains a project list that fails to load", async () => {
    contractMocks.listProjects.mockRejectedValue(new Error("upstream down"));
    setup();
    expect(await screen.findByText(CREATE_POD_PROJECTS_FAILED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: CREATE_POD_PROJECT_PLACEHOLDER })).toBeDisabled();
  });

  it("disables the picker when the workspace has no projects", async () => {
    contractMocks.listProjects.mockResolvedValue([]);
    setup();
    await waitFor(() => expect(contractMocks.listProjects).toHaveBeenCalled());
    expect(await screen.findByRole("button", { name: CREATE_POD_PROJECT_PLACEHOLDER })).toBeDisabled();
  });
});
