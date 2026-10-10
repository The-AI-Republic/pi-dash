/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

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
    EModalWidth: { XXL: "XXL" },
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

describe("SchedulerFormModal", () => {
  it("labels Description as optional and creates a template with an empty description", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={onSubmit} />);

    expect(screen.getByLabelText("Description (optional)")).toHaveValue("");

    await user.type(screen.getByLabelText("Name"), "Weekly Review");
    await user.type(screen.getByLabelText("Slug"), "weekly-review");
    await user.type(screen.getByLabelText("Prompt"), "Review the week");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        expect.objectContaining({ name: "Weekly Review", slug: "weekly-review", description: "" })
      )
    );
  });

  it("labels Description as optional in edit mode", () => {
    const scheduler = {
      id: "sched-1",
      slug: "security-audit",
      name: "Security audit",
      description: "",
      prompt: "Look",
      color: "#3b82f6",
      is_enabled: true,
    } as IScheduler;
    render(<SchedulerFormModal isOpen onClose={vi.fn()} onSubmit={vi.fn()} scheduler={scheduler} />);

    expect(screen.getByText("Edit scheduler")).toBeInTheDocument();
    expect(screen.getByLabelText("Description (optional)")).toBeInTheDocument();
  });
});
