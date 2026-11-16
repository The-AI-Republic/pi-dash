/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// vi.mock is hoisted above all imports; use vi.hoisted so the spy
// references survive that hoist and stay shared with the test body.
const { updateBinding, setToast } = vi.hoisted(() => ({
  updateBinding: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  SchedulerService: class {
    updateBinding = updateBinding;
  },
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) => (vars ? `${key}:${JSON.stringify(vars)}` : key),
  }),
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "ERROR", SUCCESS: "SUCCESS" },
  setToast,
}));

vi.mock("@pi-dash/propel/button", () => ({
  Button: ({
    children,
    loading: _loading,
    variant: _variant,
    ...props
  }: {
    children: React.ReactNode;
    loading?: boolean;
    variant?: string;
  } & React.ButtonHTMLAttributes<HTMLButtonElement>) => <button {...props}>{children}</button>,
}));

// Replace the heavy UI lib with plain DOM equivalents. Unlike the
// new-scheduler-modal suite, BindingScheduleFields is NOT stubbed here —
// these tests exercise the real recurrence builder inside the modal.
vi.mock("@pi-dash/ui", async () => {
  const { forwardRef: fwd } = await import("react");
  return {
    ModalCore: ({ isOpen, children }: { isOpen: boolean; children: React.ReactNode }) =>
      isOpen ? <div role="dialog">{children}</div> : null,
    EModalPosition: { CENTER: "CENTER" },
    EModalWidth: { XL: "XL", XXL: "XXL" },
    TextArea: fwd<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement> & { hasError?: boolean }>(
      function TextArea({ hasError: _hasError, ...props }, ref) {
        return <textarea ref={ref} {...props} />;
      }
    ),
    ToggleSwitch: ({ value, onChange }: { value: boolean; onChange: (v: boolean) => void }) => (
      <input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} />
    ),
  };
});

vi.mock("@/components/project/scheduler-bindings/binding-outcome-mode-field", () => ({
  BindingOutcomeModeField: () => null,
  DEFAULT_OUTCOME_MODE: "create_issue",
}));
vi.mock("@/components/project/scheduler-bindings/binding-pod-field", () => ({
  BindingPodField: () => null,
}));

import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import type { ISchedulerBinding } from "@pi-dash/services";

// Monday, January 5 2026, 18:00 UTC — far from midnight in any test-runner
// timezone, so the local weekday the widget derives stays Monday.
const BINDING = {
  id: "bind-1",
  scheduler: "sched-1",
  scheduler_name: "Security audit",
  dtstart: "2026-01-05T18:00:00Z",
  tzid: "UTC",
  rrule: "FREQ=DAILY",
  extra_context: "",
  enabled: true,
  outcome_mode: "create_issue",
  pod: null,
} as unknown as ISchedulerBinding;

function renderModal(binding: ISchedulerBinding = BINDING) {
  const onClose = vi.fn();
  const onUpdated = vi.fn();
  const utils = render(
    <EditSchedulerBindingModal
      isOpen
      onClose={onClose}
      workspaceSlug="acme"
      projectId="proj-1"
      binding={binding}
      onUpdated={onUpdated}
    />
  );
  return { ...utils, onClose, onUpdated };
}

async function submitAndGetPayload() {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(updateBinding).toHaveBeenCalledTimes(1));
  return updateBinding.mock.calls[0][3] as { rrule: string };
}

describe("EditSchedulerBindingModal — recurrence builder", () => {
  beforeEach(() => {
    updateBinding.mockReset();
    updateBinding.mockResolvedValue({ ...BINDING });
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("reconstructs widget state from the stored rule", () => {
    renderModal({ ...BINDING, rrule: "FREQ=WEEKLY;BYDAY=MO,WE" } as ISchedulerBinding);
    expect(screen.getByLabelText("Repeat every")).toHaveValue("WEEKLY");
    expect(screen.getByRole("button", { name: "Monday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Wednesday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Friday" })).toHaveAttribute("aria-pressed", "false");
    // Builder mode: the raw textarea stays behind the Advanced disclosure.
    expect(screen.queryByDisplayValue("FREQ=WEEKLY;BYDAY=MO,WE")).toBeNull();
  });

  it("submits the stored rule byte-identical when nothing is touched", async () => {
    renderModal({ ...BINDING, rrule: "FREQ=WEEKLY;BYDAY=MO,WE" } as ISchedulerBinding);
    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("FREQ=WEEKLY;BYDAY=MO,WE");
  });

  it("opens a rule the widget can't express in raw mode and submits it byte-identical", async () => {
    const rule = "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9;BYMINUTE=0";
    renderModal({ ...BINDING, rrule: rule } as ISchedulerBinding);
    // No builder controls; the raw textarea is forced open with the exact rule.
    expect(screen.queryByLabelText("Repeat every")).toBeNull();
    expect(screen.getByDisplayValue(rule)).toBeInTheDocument();

    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe(rule);
  });

  it("builds a weekly rule with chips and an occurrence count from the widget", async () => {
    const user = userEvent.setup();
    renderModal(); // stored FREQ=DAILY

    await user.selectOptions(screen.getByLabelText("Repeat every"), "WEEKLY");
    // dtstart is a Monday, so MO is pre-selected; add Friday.
    await user.click(screen.getByRole("button", { name: "Friday" }));

    // Ends: After [13] occurrences.
    const radios = screen.getAllByRole("radio");
    await user.click(radios[2]);
    const count = screen.getByLabelText("Number of occurrences");
    await user.clear(count);
    await user.type(count, "13");

    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("FREQ=WEEKLY;BYDAY=MO,FR;COUNT=13");
  });

  it("builds an UNTIL rule serialized as inclusive end-of-day UTC", async () => {
    const user = userEvent.setup();
    renderModal();

    const radios = screen.getAllByRole("radio");
    await user.click(radios[1]);
    const date = screen.getByLabelText("End date (UTC)");
    // Clearing then typing a full date; the input is type=date.
    await user.clear(date);
    await user.type(date, "2026-12-26");

    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("FREQ=DAILY;UNTIL=20261226T235959Z");
  });

  it("reaches the single-shot state (empty rrule) through the widget", async () => {
    const user = userEvent.setup();
    renderModal();

    await user.selectOptions(screen.getByLabelText("Repeat every"), "NONE");
    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("");
  });

  it("round-trips an untouched single-shot binding", async () => {
    renderModal({ ...BINDING, rrule: "" } as ISchedulerBinding);
    expect(screen.getByLabelText("Repeat every")).toHaveValue("NONE");
    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("");
  });

  it("keeps the widget in sync when the raw textarea is edited to an expressible rule", async () => {
    const user = userEvent.setup();
    renderModal();

    await user.click(screen.getByRole("button", { name: "Advanced: edit raw RRULE" }));
    const textarea = screen.getByDisplayValue("FREQ=DAILY");
    await user.clear(textarea);
    await user.type(textarea, "FREQ=MONTHLY;BYMONTHDAY=15");

    expect(screen.getByLabelText("Repeat every")).toHaveValue("MONTHLY");
    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("FREQ=MONTHLY;BYMONTHDAY=15");
  });

  it("guards the interval input at min 1", async () => {
    const user = userEvent.setup();
    renderModal();

    const interval = screen.getByLabelText("Interval");
    await user.clear(interval);
    await user.type(interval, "0");
    // The widget clamps to 1, so INTERVAL is omitted entirely.
    const payload = await submitAndGetPayload();
    expect(payload.rrule).toBe("FREQ=DAILY");
  });
});
