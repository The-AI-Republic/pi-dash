/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { IScheduler } from "@pi-dash/services";

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) => (vars ? `${key}:${JSON.stringify(vars)}` : key),
  }),
}));

vi.mock("@pi-dash/propel/button", () => ({
  // Strip non-DOM props (`loading`, `variant`) so React doesn't warn about
  // forwarding unknown booleans to the native <button>.
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

// Replace the heavy UI lib with plain DOM equivalents so the modal can be
// driven with userEvent without dragging in popper/headlessui machinery.
vi.mock("@pi-dash/ui", async () => {
  const { forwardRef: fwd } = await import("react");
  return {
    ModalCore: ({ isOpen, children }: { isOpen: boolean; children: React.ReactNode }) =>
      isOpen ? <div role="dialog">{children}</div> : null,
    EModalPosition: { CENTER: "CENTER" },
    EModalWidth: { XL: "XL", XXL: "XXL" },
    Input: fwd<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement> & { hasError?: boolean }>(function Input(
      { hasError: _hasError, ...props },
      ref
    ) {
      return <input ref={ref} {...props} />;
    }),
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

import { SchedulerFormModal } from "@/components/schedulers/scheduler-form-modal";

const makeScheduler = (overrides: Partial<IScheduler> = {}): IScheduler => ({
  id: "sched-1",
  workspace: "ws-1",
  slug: "weekly-digest",
  name: "Weekly digest",
  description: "",
  prompt: "Summarise the week.",
  color: "#3b82f6",
  source: "builtin",
  is_builtin: false,
  is_enabled: true,
  active_binding_count: 1,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...overrides,
});

const slugInput = () => document.getElementById("scheduler-slug") as HTMLInputElement;
const nameInput = () => document.getElementById("scheduler-name") as HTMLInputElement;

const renderModal = (scheduler: IScheduler | null, onSubmit = vi.fn().mockResolvedValue(undefined)) => {
  render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={onSubmit} scheduler={scheduler} />);
  return onSubmit;
};

describe("SchedulerFormModal slug field", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("is enabled when editing a user-created scheduler and submits the edited slug", async () => {
    const user = userEvent.setup();
    const onSubmit = renderModal(makeScheduler());

    expect(slugInput()).not.toBeDisabled();
    expect(slugInput()).toHaveValue("weekly-digest");
    expect(screen.queryByText("Built-in schedulers keep their slug.")).not.toBeInTheDocument();

    await user.clear(slugInput());
    await user.type(slugInput(), "monthly-digest");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).toMatchObject({ slug: "monthly-digest", name: "Weekly digest" });
  });

  it("is disabled with a reason when editing a built-in scheduler", () => {
    renderModal(makeScheduler({ slug: "security-audit", is_builtin: true }));

    expect(slugInput()).toBeDisabled();
    expect(slugInput()).toHaveValue("security-audit");
    expect(screen.getByText("Built-in schedulers keep their slug.")).toBeInTheDocument();
  });

  it("leaves the slug alone when only the name changes", async () => {
    const user = userEvent.setup();
    const onSubmit = renderModal(makeScheduler());

    await user.clear(nameInput());
    await user.type(nameInput(), "Monthly digest");
    expect(slugInput()).toHaveValue("weekly-digest");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).toMatchObject({ slug: "weekly-digest", name: "Monthly digest" });
  });

  it("applies the create-time format rule on edit", async () => {
    const user = userEvent.setup();
    const onSubmit = renderModal(makeScheduler());

    await user.clear(slugInput());
    await user.type(slugInput(), "Not A Slug");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Use lowercase letters, numbers, and dashes only.")).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("shows a duplicate-slug API error under the slug field", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockRejectedValue({ slug: ["A scheduler with this slug already exists."] });
    renderModal(makeScheduler(), onSubmit);

    await user.clear(slugInput());
    await user.type(slugInput(), "taken-slug");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("A scheduler with this slug already exists.")).toBeInTheDocument();
    expect(slugInput()).toHaveValue("taken-slug");
  });
});
