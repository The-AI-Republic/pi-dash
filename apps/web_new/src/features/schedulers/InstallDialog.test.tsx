// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    createScheduler: vi.fn(),
    createSchedulerBinding: vi.fn(),
    listSchedulers: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  InstallDialog,
  InstallForm,
  definitionFieldError,
  failureMessage,
  initialInstallMode,
  validateDefinitionDraft,
  type InstallDialogProps,
} from "./InstallDialog.js";

const SLUG = "acme";
const PROJECT = "p1";

export function scheduler(overrides: Partial<Scheduler> = {}): Scheduler {
  return {
    id: "s1",
    workspace: "w1",
    slug: "weekly-review",
    name: "Weekly review",
    description: "Look at the week.",
    prompt: "Review the week",
    color: "#5e6ad2",
    source: "builtin",
    is_enabled: true,
    active_binding_count: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

/** Timing half of an install row; kept apart so a test overrides one at a time. */
function runState(overrides: Record<string, unknown> = {}) {
  return {
    rrule: "FREQ=DAILY",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "UTC",
    enabled: true,
    next_run_at: null,
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    rdates: [] as string[],
    exdates: [] as string[],
    actor: null,
    ...overrides,
  };
}

function binding(overrides: Record<string, unknown> = {}) {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#5e6ad2",
    project: PROJECT,
    workspace: "w1",
    outcome_mode: "create_issue",
    extra_context: "",
    pod: null,
    pod_name: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...runState(),
    ...overrides,
  };
}

function renderForm(
  options: Partial<InstallDialogProps> & {
    installable?: Scheduler[];
    onInstalled?: () => void;
    onClose?: () => void;
  }
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onClose = options.onClose ?? vi.fn();
  const onInstalled = options.onInstalled ?? vi.fn();
  const refreshDefinitions = vi.fn(async () => undefined);
  render(
    <QueryClientProvider client={queryClient}>
      <InstallForm
        workspaceSlug={SLUG}
        projectId={PROJECT}
        canInstall={options.canInstall ?? true}
        canAuthor={options.canAuthor ?? true}
        installable={options.installable ?? [scheduler()]}
        onClose={onClose}
        onInstalled={onInstalled}
        refreshDefinitions={refreshDefinitions}
      />
    </QueryClientProvider>
  );
  return { onClose, onInstalled, refreshDefinitions, user: userEvent.setup() };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("install path rules", () => {
  it("picks the opening path from eligibility and author rights", () => {
    expect(initialInstallMode(true, 2)).toBe("install");
    expect(initialInstallMode(true, 0)).toBe("create");
    expect(initialInstallMode(false, 0)).toBe("dead-end");
  });

  it("validates the authored draft against the definition rules", () => {
    expect(validateDefinitionDraft({ name: "", handle: "", description: "", prompt: "", color: "nope" })).toEqual({
      name: "Enter a scheduler name.",
      handle: "Enter a handle.",
      prompt: "Enter the base prompt.",
      color: "Choose a six-digit hex color.",
    });
    expect(
      validateDefinitionDraft({
        name: "Weekly",
        handle: "weekly",
        description: "",
        prompt: "Review",
        color: "#5e6ad2",
      })
    ).toEqual({});
  });

  it("falls back for a bare or malformed failure", () => {
    expect(failureMessage("boom")).toBe("Check your connection and try again.");
    expect(failureMessage(new ApiError({ status: 500, code: "server_error", message: "Server exploded" }))).toBe(
      "Server exploded"
    );
    expect(failureMessage(new ApiError({ status: 500, code: "server_error", message: "  " }))).toBe(
      "The install was refused."
    );
  });

  it("reads server field detail and failure messages", () => {
    const error = new ApiError({
      status: 400,
      code: "invalid",
      message: "Bad Request",
      fields: { slug: ["This slug is already in use in this workspace."] },
    });
    expect(definitionFieldError(error, "slug")).toBe("This slug is already in use in this workspace.");
    expect(definitionFieldError(error, "name")).toBeUndefined();
    expect(definitionFieldError(new Error("x"), "slug")).toBeUndefined();
    expect(
      failureMessage(new ApiError({ status: 400, code: "invalid", message: "", fields: { rrule: ["Bad rule"] } }))
    ).toBe("Bad rule");
    expect(failureMessage(new ApiError({ status: 400, code: "invalid", message: "Refused", fields: {} }))).toBe(
      "Refused"
    );
    expect(failureMessage(new ApiError({ status: 400, code: "invalid", message: "  ", fields: {} }))).toBe(
      "The install was refused."
    );
    expect(failureMessage(new ApiError({ status: 400, code: "invalid", message: "", fields: { dtstart: [] } }))).toBe(
      "The install was refused."
    );
    expect(failureMessage(new Error("x"))).toBe("Check your connection and try again.");
  });
});

describe("InstallForm install path", () => {
  it("offers only the eligible definitions, labelled name and handle", async () => {
    renderForm({ canAuthor: false, installable: [scheduler(), scheduler({ id: "s2", slug: "other", name: "Other" })] });
    const select = screen.getByLabelText("Scheduler");
    expect(select.tagName).toBe("SELECT");
    expect(screen.getByRole("option", { name: "Weekly review (weekly-review)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Other (other)" })).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.getByText(/Ask a workspace admin to add more/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeEnabled();
  });

  it("keeps the submit inert without install rights", async () => {
    renderForm({ canAuthor: false, canInstall: false });
    expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
  });

  it("refuses a missing start and keeps the dialog open", async () => {
    renderForm({ canAuthor: false });
    const { user } = { user: userEvent.setup() };
    await user.clear(screen.getByLabelText("Starts at"));
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(await screen.findByText("Start time is required.")).toBeInTheDocument();
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
  });

  it("installs with the schedule and closes", async () => {
    contracts.createSchedulerBinding.mockResolvedValue(binding());
    const { onInstalled, onClose, user } = renderForm({ canAuthor: false });
    await user.selectOptions(screen.getByLabelText("Time zone"), "America/New_York");
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(onInstalled).toHaveBeenCalled();
    expect(notifySuccess).toHaveBeenCalledWith("Scheduler installed", "It will fire on the configured schedule.");
    const [, slug, projectId, input] = contracts.createSchedulerBinding.mock.calls[0] as unknown[];
    expect([slug, projectId]).toEqual([SLUG, PROJECT]);
    expect(input).toMatchObject({
      scheduler: "s1",
      tzid: "America/New_York",
      rrule: "FREQ=DAILY",
      enabled: true,
      outcome_mode: "create_issue",
    });
  });

  it("surfaces a refused install and stays open", async () => {
    contracts.createSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 400, code: "invalid", message: "Bad Request", fields: { rrule: ["Rule is invalid."] } })
    );
    const { onClose, user } = renderForm({ canAuthor: false });
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Rule is invalid."));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("ignores a second submit while the write is in flight", async () => {
    contracts.createSchedulerBinding.mockReturnValue(new Promise(() => undefined));
    const { user } = renderForm({ canAuthor: false });
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Installing…" })).toBeInTheDocument());
    fireEvent.submit(screen.getByRole("form", { name: "New Scheduler" }));
    await waitFor(() => expect(contracts.createSchedulerBinding).toHaveBeenCalledTimes(1));
  });

  it("does not submit without install rights", async () => {
    const { user } = renderForm({ canAuthor: false, canInstall: false });
    fireEvent.submit(screen.getByRole("form", { name: "New Scheduler" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
  });

  it("disables the submit while a write is in flight", async () => {
    contracts.createSchedulerBinding.mockReturnValue(new Promise(() => undefined));
    const { user } = renderForm({ canAuthor: false });
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Installing…" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });
});

describe("InstallForm author path", { timeout: 30_000 }, () => {
  it("creates and installs in one submit", async () => {
    contracts.createScheduler.mockResolvedValue(scheduler({ id: "new", slug: "authored" }));
    contracts.createSchedulerBinding.mockResolvedValue(binding({ scheduler: "new", scheduler_slug: "authored" }));
    const { onClose, refreshDefinitions, user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.type(screen.getByLabelText("Name"), "Author ed");
    expect(screen.getByLabelText("Slug")).toHaveValue("author-ed");
    await user.clear(screen.getByLabelText("Slug"));
    await user.type(screen.getByLabelText("Slug"), "authored");
    await user.type(screen.getByLabelText("Prompt"), "Audit nightly");
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.createScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, {
      name: "Author ed",
      slug: "authored",
      description: "",
      prompt: "Audit nightly",
      color: "#5e6ad2",
      is_enabled: true,
    });
    expect(refreshDefinitions).toHaveBeenCalled();
    expect(notifySuccess).toHaveBeenCalledWith(
      "Scheduler created and installed",
      "It will fire on the configured schedule."
    );
  });

  it("maps a handle conflict inline and stores nothing", async () => {
    contracts.createScheduler.mockRejectedValue(
      new ApiError({
        status: 400,
        code: "invalid",
        message: "Bad Request",
        fields: { slug: ["This slug is already in use in this workspace."] },
      })
    );
    const { user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.type(screen.getByLabelText("Name"), "Clash");
    await user.type(screen.getByLabelText("Prompt"), "Run");
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    expect(await screen.findByText("This slug is already in use in this workspace.")).toBeInTheDocument();
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports any other create failure as a notice", async () => {
    contracts.createScheduler.mockRejectedValue(
      new ApiError({ status: 500, code: "server_error", message: "Server exploded", fields: {} })
    );
    const { user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.type(screen.getByLabelText("Name"), "Any");
    await user.type(screen.getByLabelText("Prompt"), "Run");
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Server exploded"));
  });

  it("rejects an invalid draft before any write", async () => {
    const { user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    expect(await screen.findByText("Enter a scheduler name.")).toBeInTheDocument();
    expect(screen.getByText("Enter the base prompt.")).toBeInTheDocument();
    expect(contracts.createScheduler).not.toHaveBeenCalled();
  });

  it("flips to the install path with the fresh definition picked when the install fails", async () => {
    contracts.createScheduler.mockResolvedValue(scheduler({ id: "fresh", slug: "flipped", name: "Flipped" }));
    contracts.createSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 400, code: "invalid", message: "Bad Request", fields: { rrule: ["Bad"] } })
    );
    const { onClose, user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.type(screen.getByLabelText("Name"), "Flipped");
    await user.type(screen.getByLabelText("Prompt"), "Run");
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Scheduler created but not installed",
        expect.stringContaining("workspace catalog")
      )
    );
    expect(await screen.findByRole("tab", { name: "Install existing" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Scheduler")).toHaveValue("fresh");
    expect(onClose).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(contracts.createSchedulerBinding).toHaveBeenCalledTimes(2));
  });

  it("explains an empty install path when the user switches to it", async () => {
    const { user } = renderForm({ canAuthor: true, installable: [] });
    expect(screen.getByRole("tab", { name: "Create new" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("tab", { name: "Install existing" }));
    expect(screen.getByText(/Every enabled workspace scheduler is already installed/)).toBeInTheDocument();
    expect(screen.queryByLabelText("Scheduler")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
  });

  it("stops deriving the handle once the user edits it", async () => {
    const { user } = renderForm({ canAuthor: true });
    await user.click(screen.getByRole("tab", { name: "Create new" }));
    await user.clear(screen.getByLabelText("Slug"));
    await user.type(screen.getByLabelText("Slug"), "my-own-handle");
    await user.type(screen.getByLabelText("Name"), "Weekly");
    expect(screen.getByLabelText("Slug")).toHaveValue("my-own-handle");
  });

  it("shows the working label while the author path is in flight", async () => {
    contracts.createScheduler.mockReturnValue(new Promise(() => undefined));
    const { user } = renderForm({ canAuthor: true, installable: [] });
    const prompt = screen.getByLabelText("Prompt");
    await user.type(screen.getByLabelText("Name"), "Slow");
    await user.type(prompt, "Run");
    await user.click(screen.getByRole("button", { name: "Create & install" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Creating & installing…" })).toBeDisabled());
  });

  it("leaves the author-only dialog on create when there is nothing installable and can author", () => {
    renderForm({ canAuthor: true, installable: [] });
    expect(screen.getByRole("tab", { name: "Create new" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByLabelText("Scheduler")).not.toBeInTheDocument();
  });

  it("offers the dead end without author rights", () => {
    renderForm({ canAuthor: false, canInstall: false, installable: [] });
    expect(screen.getByText("No schedulers available")).toBeInTheDocument();
    expect(screen.getByText(/Ask a workspace admin/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
  });
});

describe("InstallDialog", () => {
  it("filters installed definitions out of the picker and opens on create when none remain", async () => {
    contracts.listSchedulers.mockResolvedValue([scheduler(), scheduler({ id: "s2", slug: "off", is_enabled: false })]);
    const onClose = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <InstallDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          open
          onClose={onClose}
          canInstall
          canAuthor
          installedSchedulerIds={["s1"]}
          onInstalled={vi.fn()}
        />
      </QueryClientProvider>
    );
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Create new" })).toHaveAttribute("aria-selected", "true")
    );
    expect(screen.queryByLabelText("Scheduler")).not.toBeInTheDocument();
  });

  it("asks the caller to close when the dialog is dismissed", async () => {
    contracts.listSchedulers.mockResolvedValue([scheduler()]);
    const onClose = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={queryClient}>
        <InstallDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          open
          onClose={onClose}
          canInstall
          canAuthor
          installedSchedulerIds={[]}
          onInstalled={vi.fn()}
        />
      </QueryClientProvider>
    );
    await screen.findByRole("form", { name: "New Scheduler" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("does not mount the form while closed", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <InstallDialog
          workspaceSlug={SLUG}
          projectId={PROJECT}
          open={false}
          onClose={vi.fn()}
          canInstall
          canAuthor
          installedSchedulerIds={[]}
          onInstalled={vi.fn()}
        />
      </QueryClientProvider>
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(contracts.listSchedulers).not.toHaveBeenCalled();
  });
});
