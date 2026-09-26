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
const { createScheduler, createBinding, setToast } = vi.hoisted(() => ({
  createScheduler: vi.fn(),
  createBinding: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  SchedulerService: class {
    createScheduler = createScheduler;
    createBinding = createBinding;
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

// The binding sub-forms carry their own coverage; stub them so these tests
// exercise the mode switch + submit orchestration only.
vi.mock("@/components/project/scheduler-bindings/binding-schedule-fields", () => ({
  BindingScheduleFields: () => null,
}));
vi.mock("@/components/project/scheduler-bindings/binding-outcome-mode-field", () => ({
  BindingOutcomeModeField: () => null,
  DEFAULT_OUTCOME_MODE: "create_issue",
}));
vi.mock("@/components/project/scheduler-bindings/binding-pod-field", () => ({
  BindingPodField: () => null,
}));

import { NewSchedulerModal } from "@/components/project/scheduler-bindings/new-scheduler-modal";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";

const SCHEDULERS = [
  { id: "sched-1", slug: "security-audit", name: "Security audit", is_enabled: true },
  { id: "sched-2", slug: "gdpr", name: "GDPR", is_enabled: true },
  { id: "sched-disabled", slug: "off", name: "Disabled one", is_enabled: false },
] as IScheduler[];

describe("NewSchedulerModal", () => {
  beforeEach(() => {
    createScheduler.mockReset();
    createBinding.mockReset();
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal(overrides: Partial<React.ComponentProps<typeof NewSchedulerModal>> = {}) {
    const onClose = vi.fn();
    const onInstalled = vi.fn();
    const onSchedulersChanged = vi.fn();
    const utils = render(
      <NewSchedulerModal
        isOpen
        onClose={onClose}
        workspaceSlug="acme"
        projectId="proj-1"
        availableSchedulers={SCHEDULERS}
        existingBindings={[] as ISchedulerBinding[]}
        canCreateScheduler
        onInstalled={onInstalled}
        onSchedulersChanged={onSchedulersChanged}
        {...overrides}
      />
    );
    return { ...utils, onClose, onInstalled, onSchedulersChanged };
  }

  describe("default mode selection", () => {
    it("defaults to Install existing when installable schedulers exist", () => {
      renderModal();
      expect(screen.getByRole("tab", { name: "Install existing" })).toHaveAttribute("aria-selected", "true");
      expect(screen.getByLabelText("Scheduler")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Install" })).toBeInTheDocument();
    });

    it("defaults to Create new when nothing is installable and the user may create", () => {
      renderModal({ availableSchedulers: [] });
      expect(screen.getByRole("tab", { name: "Create new" })).toHaveAttribute("aria-selected", "true");
      expect(screen.getByLabelText("Name")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Create & install" })).toBeInTheDocument();
    });

    it("treats already-bound and disabled schedulers as not installable", () => {
      renderModal({
        availableSchedulers: SCHEDULERS,
        existingBindings: [{ scheduler: "sched-1" }, { scheduler: "sched-2" }] as ISchedulerBinding[],
      });
      expect(screen.getByRole("tab", { name: "Create new" })).toHaveAttribute("aria-selected", "true");
    });
  });

  describe("mode switch", () => {
    it("switches between install and create forms", async () => {
      const user = userEvent.setup();
      renderModal();

      await user.click(screen.getByRole("tab", { name: "Create new" }));
      expect(screen.getByLabelText("Name")).toBeInTheDocument();
      expect(screen.queryByLabelText("Scheduler")).toBeNull();
      expect(screen.getByRole("button", { name: "Create & install" })).toBeInTheDocument();

      await user.click(screen.getByRole("tab", { name: "Install existing" }));
      expect(screen.getByLabelText("Scheduler")).toBeInTheDocument();
      expect(screen.queryByLabelText("Name")).toBeNull();
    });

    it("visiting the create tab does not block a later install submit with hidden required fields", async () => {
      const user = userEvent.setup();
      createBinding.mockResolvedValue({ id: "bind-1" });
      const { onInstalled, onClose } = renderModal();

      await user.click(screen.getByRole("tab", { name: "Create new" }));
      await user.click(screen.getByRole("tab", { name: "Install existing" }));
      await user.click(screen.getByRole("button", { name: "Install" }));

      await waitFor(() => {
        expect(createBinding).toHaveBeenCalledWith(
          "acme",
          "proj-1",
          expect.objectContaining({ scheduler: "sched-1", project: "proj-1" })
        );
      });
      expect(createScheduler).not.toHaveBeenCalled();
      expect(onInstalled).toHaveBeenCalledWith({ id: "bind-1" });
      expect(onClose).toHaveBeenCalled();
    });
  });

  describe("permission gating", () => {
    it("hides the Create new tab and shows a hint for non-workspace-admins", () => {
      renderModal({ canCreateScheduler: false });
      expect(screen.queryByRole("tab", { name: "Create new" })).toBeNull();
      expect(screen.getByLabelText("Scheduler")).toBeInTheDocument();
      expect(screen.getByText(/Ask a workspace admin to add schedulers to the catalog\./)).toBeInTheDocument();
    });

    it("shows the empty-state dead end when nothing is installable and the user cannot create", () => {
      renderModal({ canCreateScheduler: false, availableSchedulers: [] });
      expect(screen.getByText("No schedulers available")).toBeInTheDocument();
      expect(screen.queryByRole("tab", { name: "Create new" })).toBeNull();
      expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
    });
  });

  describe("create path", () => {
    it("auto-derives the slug from the name until the slug is edited", async () => {
      const user = userEvent.setup();
      renderModal({ availableSchedulers: [] });

      await user.type(screen.getByLabelText("Name"), "Weekly Review");
      expect(screen.getByLabelText("Slug")).toHaveValue("weekly-review");

      await user.clear(screen.getByLabelText("Slug"));
      await user.type(screen.getByLabelText("Slug"), "custom");
      await user.type(screen.getByLabelText("Name"), " 2");
      expect(screen.getByLabelText("Slug")).toHaveValue("custom");
    });

    it("creates the definition then the binding, and closes on success", async () => {
      const user = userEvent.setup();
      createScheduler.mockResolvedValue({ id: "sched-new", slug: "weekly-review" });
      createBinding.mockResolvedValue({ id: "bind-new" });
      const { onInstalled, onClose, onSchedulersChanged } = renderModal({ availableSchedulers: [] });

      await user.type(screen.getByLabelText("Name"), "Weekly Review");
      await user.type(screen.getByLabelText("Prompt"), "Review the week");
      await user.click(screen.getByRole("button", { name: "Create & install" }));

      await waitFor(() => {
        expect(createScheduler).toHaveBeenCalledWith("acme", {
          slug: "weekly-review",
          name: "Weekly Review",
          description: "",
          prompt: "Review the week",
          color: expect.any(String),
          is_enabled: true,
        });
      });
      await waitFor(() => {
        expect(createBinding).toHaveBeenCalledWith(
          "acme",
          "proj-1",
          expect.objectContaining({ scheduler: "sched-new", project: "proj-1" })
        );
      });
      expect(onSchedulersChanged).toHaveBeenCalled();
      expect(onInstalled).toHaveBeenCalledWith({ id: "bind-new" });
      expect(onClose).toHaveBeenCalled();
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "SUCCESS", title: "Scheduler created and installed" })
      );
    });

    it("blocks submit when required create fields are empty", async () => {
      const user = userEvent.setup();
      renderModal({ availableSchedulers: [] });

      await user.click(screen.getByRole("button", { name: "Create & install" }));

      expect(createScheduler).not.toHaveBeenCalled();
      expect(await screen.findByText("Name is required.")).toBeInTheDocument();
      expect(screen.getByText("Slug is required.")).toBeInTheDocument();
      expect(screen.getByText("Prompt is required.")).toBeInTheDocument();
    });

    it("maps a slug 400 from the serializer onto the slug field inline", async () => {
      const user = userEvent.setup();
      // Real wire shape: SchedulerSerializer.validate_slug returns 400
      // {"slug": ["This slug is already in use in this workspace."]} and the
      // service rethrows err.response.data.
      createScheduler.mockRejectedValue({ slug: ["This slug is already in use in this workspace."] });
      const { onClose } = renderModal({ availableSchedulers: [] });

      await user.type(screen.getByLabelText("Name"), "Weekly Review");
      await user.type(screen.getByLabelText("Prompt"), "Review the week");
      await user.click(screen.getByRole("button", { name: "Create & install" }));

      expect(await screen.findByText("This slug is already in use in this workspace.")).toBeInTheDocument();
      expect(createBinding).not.toHaveBeenCalled();
      expect(setToast).not.toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();
    });

    it("surfaces the partial failure (definition created, binding failed) and flips to install mode", async () => {
      const user = userEvent.setup();
      createScheduler.mockResolvedValue({ id: "sched-new", slug: "weekly-review" });
      createBinding.mockRejectedValue({ error: "boom" });
      const { onInstalled, onClose, onSchedulersChanged } = renderModal({ availableSchedulers: [] });

      await user.type(screen.getByLabelText("Name"), "Weekly Review");
      await user.type(screen.getByLabelText("Prompt"), "Review the week");
      await user.click(screen.getByRole("button", { name: "Create & install" }));

      await waitFor(() => {
        expect(setToast).toHaveBeenCalledWith(
          expect.objectContaining({ type: "ERROR", title: "Scheduler created but not installed" })
        );
      });
      // Catalog refreshed so the new definition shows in the install picker on retry.
      expect(onSchedulersChanged).toHaveBeenCalled();
      expect(onInstalled).not.toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();
      // Modal flipped to the install path for the retry.
      expect(screen.getByRole("tab", { name: "Install existing" })).toHaveAttribute("aria-selected", "true");
    });
  });

  describe("install path", () => {
    it("shows the toast detail from the backend on install failure and keeps the modal open", async () => {
      const user = userEvent.setup();
      createBinding.mockRejectedValue({ rrule: ["Invalid RRULE."] });
      const { onClose } = renderModal();

      await user.click(screen.getByRole("button", { name: "Install" }));

      await waitFor(() => {
        expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "ERROR", message: "Invalid RRULE." }));
      });
      expect(onClose).not.toHaveBeenCalled();
    });

    it("disables submit on the install tab when nothing is installable", async () => {
      const user = userEvent.setup();
      renderModal({ availableSchedulers: [] });

      await user.click(screen.getByRole("tab", { name: "Install existing" }));
      expect(
        screen.getByText(
          "Every enabled workspace scheduler is already installed on this project. Create a new one instead."
        )
      ).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
    });
  });
});
