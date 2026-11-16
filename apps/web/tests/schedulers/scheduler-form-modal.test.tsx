/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
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
import type { IScheduler } from "@pi-dash/services";

const DUPLICATE = "A scheduler with the same name already exists in this workspace.";

const EXISTING = {
  id: "sched-1",
  slug: "nightly-bug-scan",
  name: "Nightly Bug Scan",
  description: "",
  prompt: "Scan for bugs",
  color: "#3b82f6",
  is_enabled: true,
} as IScheduler;

describe("SchedulerFormModal", () => {
  it.each([
    ["create", null],
    ["edit", EXISTING],
  ])("shows no slug input in %s mode", (_mode, scheduler) => {
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={vi.fn()} scheduler={scheduler} />);
    expect(screen.getByLabelText("Name")).toBeInTheDocument();
    expect(screen.queryByLabelText("Slug")).toBeNull();
  });

  it("submits the definition without a slug", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={onSubmit} />);

    await user.type(screen.getByLabelText("Name"), "Nightly Bug Scan");
    await user.type(screen.getByLabelText("Prompt"), "Scan for bugs");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty("slug");
    expect(onSubmit.mock.calls[0][0]).toMatchObject({ name: "Nightly Bug Scan", prompt: "Scan for bugs" });
  });

  it("shows a duplicate-name rejection under the Name field", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockRejectedValue({ name: [DUPLICATE] });
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={onSubmit} scheduler={EXISTING} />);

    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "Security Audit");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(DUPLICATE)).toBeInTheDocument();
  });

  it("rejects a name with no letter or digit before submitting", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={onSubmit} />);

    await user.type(screen.getByLabelText("Name"), "日本語");
    await user.type(screen.getByLabelText("Prompt"), "Scan for bugs");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));

    expect(await screen.findByText("Name must contain at least one letter or digit.")).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
