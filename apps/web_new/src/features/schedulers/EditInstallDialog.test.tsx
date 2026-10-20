// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edit-install dialog (parity/specs/agents.md §3, AGT-010): every stored
// field pre-fills, the start converts from UTC to local, the recurrence
// builder reconstructs from the stored rule (raw when it cannot), save
// sends one PATCH and refreshes the caller, and a refusal keeps the dialog
// open with the typed values intact.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts, notify } = vi.hoisted(() => ({
  contracts: { listSchedulerBindings: vi.fn(), listPods: vi.fn(), updateSchedulerBinding: vi.fn() },
  notify: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: notify.error, notifySuccess: notify.success }));

import { EditInstallDialog, installDraftFromBinding, installUpdatePayload } from "./EditInstallDialog.js";
import { bindingKeys, invalidateInstalls } from "./installData.js";
import { DEFAULT_TZID } from "./schedule.js";

const SLUG = "acme";
const PROJECT = "p1";

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#22c55e",
    project: PROJECT,
    workspace: "w1",
    dtstart: "2026-10-21T07:30:00Z",
    tzid: "Europe/Berlin",
    rrule: "FREQ=WEEKLY;BYDAY=MO,WE",
    rdates: [],
    exdates: [],
    extra_context: "Frame the week.",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: null,
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T12:30:00Z",
    ...overrides,
  };
}

function setup(props: { binding: SchedulerBinding | null; onSaved?: () => void }) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [props.binding ?? binding()]);
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={queryClient}>
      <EditInstallDialog
        workspaceSlug={SLUG}
        projectId={PROJECT}
        binding={props.binding}
        onClose={onClose}
        onSaved={props.onSaved}
      />
    </QueryClientProvider>
  );
  return { onClose, queryClient, invalidate, view };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  contracts.listPods.mockResolvedValue([]);
  contracts.updateSchedulerBinding.mockResolvedValue({ ...binding(), tzid: "Asia/Tokyo" });
});
afterEach(() => {
  resetAppClient();
});

describe("installDraftFromBinding", () => {
  it("converts the stored instant to local wall time and passes the rule through", () => {
    const draft = installDraftFromBinding(binding());
    expect(draft.schedule.startsAt).toBe("2026-10-21T00:30");
    expect(draft.schedule.timeZone).toBe("Europe/Berlin");
    expect(draft.schedule.rrule).toBe("FREQ=WEEKLY;BYDAY=MO,WE");
    expect(draft.schedule.projectContext).toBe("Frame the week.");
    expect(draft.schedule.enabled).toBe(true);
    expect(draft.outcomeMode).toBe("create_issue");
    expect(draft.pod).toBeNull();
  });

  it("pre-fills the default zone for a blank stored one", () => {
    expect(installDraftFromBinding(binding({ tzid: "" })).schedule.timeZone).toBe(DEFAULT_TZID);
  });

  it("never sends a blank zone to the server", () => {
    const draft = installDraftFromBinding(binding({ tzid: "" }));
    draft.schedule.timeZone = "";
    expect(installUpdatePayload(draft).tzid).toBe(DEFAULT_TZID);
  });

  it("keeps a rule it cannot read exactly as stored", () => {
    const draft = installDraftFromBinding(binding({ rrule: "FREQ=HOURLY" }));
    expect(draft.schedule.rrule).toBe("FREQ=HOURLY");
  });
});

describe("installUpdatePayload", () => {
  it("writes every binding field and sends an empty pod as null", () => {
    const draft = installDraftFromBinding(binding({ pod: "pod1", outcome_mode: "apply_fix", enabled: false }));
    expect(installUpdatePayload(draft)).toEqual({
      dtstart: "2026-10-21T07:30:00.000Z",
      tzid: "Europe/Berlin",
      rrule: "FREQ=WEEKLY;BYDAY=MO,WE",
      extra_context: "Frame the week.",
      enabled: false,
      outcome_mode: "apply_fix",
      pod: "pod1",
    });
  });

  it("trims the context but hands the rule over untouched", () => {
    const draft = installDraftFromBinding(binding({ extra_context: "  padded  ", rrule: " FREQ=DAILY " }));
    expect(installUpdatePayload(draft)).toMatchObject({ extra_context: "padded", rrule: " FREQ=DAILY " });
  });
});

describe("EditInstallDialog", () => {
  it("renders nothing but a closed dialog without a binding", () => {
    setup({ binding: null });
    expect(screen.queryByText("Edit scheduler install")).not.toBeInTheDocument();
  });

  it("pre-fills every field from the stored install", () => {
    setup({ binding: binding() });
    expect(screen.getByText("Edit scheduler install")).toBeInTheDocument();
    expect(screen.getByLabelText("Starts at")).toHaveValue("2026-10-21T00:30");
    expect(screen.getByLabelText("Time zone")).toHaveValue("Europe/Berlin");
    expect(screen.getByLabelText("Repeat interval")).toHaveValue(1);
    expect(screen.getByLabelText("Repeat unit")).toHaveValue("week");
    expect(screen.getByRole("button", { name: "Monday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Wednesday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Project context (optional)")).toHaveValue("Frame the week.");
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Create issues" })).toBeChecked();
    expect(screen.getByLabelText("Pod")).toHaveValue("");
  });

  it("opens an inexpressible stored rule raw and submits it byte-identical", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    setup({ binding: binding({ rrule: "FREQ=HOURLY" }), onSaved });
    const raw = screen.getByLabelText("Recurrence (RRULE)");
    expect(raw).toBeVisible();
    expect(raw).toHaveValue("FREQ=HOURLY");
    expect(screen.queryByRole("button", { name: "Advanced: rule text" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      PROJECT,
      "b1",
      expect.objectContaining({ rrule: "FREQ=HOURLY" })
    );
  });

  it("keeps a stored rule byte-identical, padding included, when only other fields change", async () => {
    const user = userEvent.setup();
    setup({ binding: binding({ rrule: "FREQ=HOURLY " }) });
    expect(screen.getByLabelText("Recurrence (RRULE)")).toHaveValue("FREQ=HOURLY ");
    await user.click(screen.getByRole("radio", { name: "Apply fix" }));
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding.mock.calls[0]?.[4]).toMatchObject({ rrule: "FREQ=HOURLY " });
  });

  it("saves the full field set and settles the caller", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    const { onClose, invalidate } = setup({ binding: binding(), onSaved });
    await user.selectOptions(screen.getByLabelText("Time zone"), "Asia/Tokyo");
    await user.type(screen.getByLabelText("Project context (optional)"), " Again");
    await user.click(screen.getByRole("radio", { name: "Apply fix" }));
    await user.click(screen.getByRole("switch"));
    await user.selectOptions(screen.getByLabelText("Repeat unit"), "month");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1", {
      dtstart: "2026-10-21T07:30:00.000Z",
      tzid: "Asia/Tokyo",
      rrule: "FREQ=MONTHLY;BYMONTHDAY=21",
      extra_context: "Frame the week. Again",
      enabled: false,
      outcome_mode: "apply_fix",
      pod: null,
    });
    expect(onSaved).toHaveBeenCalledTimes(1);
    expect(notify.success).toHaveBeenCalledWith("Install updated", "Subsequent runs use the new settings.");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
  });

  it("sends the picked pod and the default when it is cleared", async () => {
    const user = userEvent.setup();
    contracts.listPods.mockResolvedValue([
      {
        id: "pod1",
        name: "RKT pool",
        description: "",
        is_default: true,
        workspace: "w1",
        project: PROJECT,
        project_identifier: "RKT",
        created_by: null,
        runner_count: 0,
        created_at: "2026-10-01T00:00:00Z",
        updated_at: "2026-10-01T00:00:00Z",
      },
    ]);
    setup({ binding: binding({ pod: "pod1" }) });
    await screen.findByRole("option", { name: "RKT pool (default)" });
    expect(screen.getByLabelText("Pod")).toHaveValue("pod1");
    await user.selectOptions(screen.getByLabelText("Pod"), "");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding.mock.calls[0]?.[4]).toMatchObject({ pod: null });
  });

  it("submits null for a saved pod the project no longer has", async () => {
    const user = userEvent.setup();
    contracts.listPods.mockResolvedValue([
      {
        id: "pod2",
        name: "Replacement pool",
        description: "",
        is_default: true,
        workspace: "w1",
        project: PROJECT,
        project_identifier: "RKT",
        created_by: null,
        runner_count: 0,
        created_at: "2026-10-01T00:00:00Z",
        updated_at: "2026-10-01T00:00:00Z",
      },
    ]);
    setup({ binding: binding({ pod: "gone" }) });
    await screen.findByRole("option", { name: "Replacement pool (default)" });
    expect(screen.getByLabelText("Pod")).toHaveValue("");
    await user.click(screen.getByRole("radio", { name: "Apply fix" }));
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding.mock.calls[0]?.[4]).toMatchObject({ pod: null });
  });

  it("falls back to the default zone when the stored one is blank", async () => {
    const user = userEvent.setup();
    setup({ binding: binding({ tzid: "" }) });
    expect(screen.getByLabelText("Time zone")).toHaveValue(DEFAULT_TZID);
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
    expect(contracts.updateSchedulerBinding.mock.calls[0]?.[4]).toMatchObject({ tzid: DEFAULT_TZID });
  });

  it("refuses an empty start locally and sends nothing", async () => {
    const user = userEvent.setup();
    setup({ binding: binding() });
    await user.clear(screen.getByLabelText("Starts at"));
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByText("Start time is required.")).toBeInTheDocument();
    expect(contracts.updateSchedulerBinding).not.toHaveBeenCalled();
  });

  it("keeps the dialog open with the typed rule when the server refuses, and re-raises a fixable rule", async () => {
    const user = userEvent.setup();
    contracts.updateSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 400, code: "invalid", message: "Bad Request", fields: { rrule: ["Unknown FREQ value."] } })
    );
    const onSaved = vi.fn();
    const { onClose } = setup({ binding: binding(), onSaved });
    await user.click(screen.getByRole("button", { name: "Advanced: rule text" }));
    await user.clear(screen.getByLabelText("Recurrence (RRULE)"));
    await user.type(screen.getByLabelText("Recurrence (RRULE)"), "FREQ=NOPE");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(notify.error).toHaveBeenCalledWith("Something went wrong", "Unknown FREQ value."));
    expect(onClose).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Recurrence (RRULE)")).toHaveValue("FREQ=NOPE");
    expect(contracts.updateSchedulerBinding.mock.calls[0]?.[4]).toMatchObject({ rrule: "FREQ=NOPE" });
  });

  it("blocks a second submit while the first is in flight", async () => {
    const user = userEvent.setup();
    let resolve: (value: SchedulerBinding) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise<SchedulerBinding>((settle) => {
        resolve = settle;
      })
    );
    setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    const saving = await screen.findByRole("button", { name: "Saving…" });
    expect(saving).toBeDisabled();
    await user.click(saving);
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(1);
    resolve(binding());
    await waitFor(() => expect(notify.success).toHaveBeenCalled());
  });

  it("refuses an implicit submit while a save is already in flight", async () => {
    const user = userEvent.setup();
    let resolve: (value: SchedulerBinding) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise<SchedulerBinding>((settle) => {
        resolve = settle;
      })
    );
    setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await screen.findByRole("button", { name: "Saving…" });
    const form = screen.getByRole("button", { name: "Saving…" }).closest("form");
    if (form === null) throw new Error("edit dialog form missing");
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(1);
    resolve(binding());
    await waitFor(() => expect(notify.success).toHaveBeenCalled());
  });

  it("asks the caller to close when Escape or the close control dismisses it", async () => {
    const user = userEvent.setup();
    const { onClose } = setup({ binding: binding() });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(2));
  });

  it("closes from the cancel control", async () => {
    const user = userEvent.setup();
    const { onClose } = setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(contracts.updateSchedulerBinding).not.toHaveBeenCalled();
  });
});

describe("invalidate helpers", () => {
  it("refreshes installs and the catalog together", async () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue();
    await invalidateInstalls(queryClient, SLUG, PROJECT);
    expect(invalidate).toHaveBeenCalledTimes(2);
  });
});
