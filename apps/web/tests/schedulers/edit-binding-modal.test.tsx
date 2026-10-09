/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// vi.mock is hoisted above all imports; use vi.hoisted so the spy
// references survive that hoist and stay shared with the test body.
const { retrieveScheduler, updateScheduler, updateBinding, setToast, mutateCache, permissions } = vi.hoisted(() => ({
  retrieveScheduler: vi.fn(),
  updateScheduler: vi.fn(),
  updateBinding: vi.fn(),
  setToast: vi.fn(),
  mutateCache: vi.fn(),
  permissions: { workspaceAdmin: true },
}));

vi.mock("@pi-dash/services", () => ({
  SchedulerService: class {
    retrieveScheduler = retrieveScheduler;
    updateScheduler = updateScheduler;
    updateBinding = updateBinding;
  },
}));

vi.mock("swr", () => ({
  useSWRConfig: () => ({ mutate: mutateCache }),
}));

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

vi.mock("@pi-dash/constants", () => ({
  EUserPermissions: { ADMIN: 20 },
  EUserPermissionsLevel: { PROJECT: "PROJECT", WORKSPACE: "WORKSPACE" },
}));

// Project admin always; workspace admin per test.
vi.mock("@/hooks/store/user", () => ({
  useUserPermissions: () => ({
    allowPermissions: (_roles: number[], level: string) => (level === "WORKSPACE" ? permissions.workspaceAdmin : true),
  }),
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
  // Strip non-DOM props so React doesn't warn about forwarding them to <button>.
  Button: ({
    children,
    loading: _loading,
    variant: _variant,
    size: _size,
    ...props
  }: {
    children: React.ReactNode;
    loading?: boolean;
    variant?: string;
    size?: string;
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

// The outcome-mode and pod fields carry their own coverage and fetch on mount.
vi.mock("@/components/project/scheduler-bindings/binding-outcome-mode-field", () => ({
  BindingOutcomeModeField: () => null,
  DEFAULT_OUTCOME_MODE: "create_issue",
}));
vi.mock("@/components/project/scheduler-bindings/binding-pod-field", () => ({
  BindingPodField: () => null,
}));

import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import type { IScheduler, ISchedulerBinding } from "@pi-dash/services";

const BINDING = {
  id: "bind-1",
  scheduler: "sched-1",
  scheduler_slug: "security-audit",
  scheduler_name: "Security audit",
  scheduler_color: "#3b82f6",
  project: "proj-1",
  dtstart: "2026-01-05T09:00:00Z",
  tzid: "UTC",
  rrule: "FREQ=WEEKLY",
  extra_context: "",
  enabled: true,
  outcome_mode: "create_issue",
  pod: null,
} as ISchedulerBinding;

const TEMPLATE = {
  id: "sched-1",
  slug: "security-audit",
  name: "Security audit",
  description: "Weekly sweep",
  prompt: "Look for security issues",
  color: "#3b82f6",
  source: "builtin",
  is_enabled: true,
  active_binding_count: 3,
} as IScheduler;

const TEMPLATE_BUTTON = "Edit scheduler template";

function renderModal(overrides: Partial<React.ComponentProps<typeof EditSchedulerBindingModal>> = {}) {
  const onClose = vi.fn();
  const onUpdated = vi.fn();
  const utils = render(
    <EditSchedulerBindingModal
      isOpen
      onClose={onClose}
      workspaceSlug="acme"
      projectId="proj-1"
      binding={BINDING}
      onUpdated={onUpdated}
      {...overrides}
    />
  );
  return { ...utils, onClose, onUpdated };
}

async function openTemplate(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));
  return screen.findByLabelText("Prompt");
}

async function retype(user: ReturnType<typeof userEvent.setup>, label: string, text: string) {
  const field = screen.getByLabelText(label);
  await user.clear(field);
  if (text) await user.type(field, text);
}

describe("EditSchedulerBindingModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    permissions.workspaceAdmin = true;
    retrieveScheduler.mockResolvedValue(TEMPLATE);
    updateBinding.mockResolvedValue({ ...BINDING, rrule: "FREQ=DAILY" });
    updateScheduler.mockImplementation(async (_slug: string, _id: string, payload: Partial<IScheduler>) => ({
      ...TEMPLATE,
      ...payload,
    }));
  });

  describe("expand / collapse", () => {
    it("starts collapsed and loads the template's current values on expand", async () => {
      const user = userEvent.setup();
      renderModal();

      expect(screen.queryByLabelText("Prompt")).toBeNull();
      expect(retrieveScheduler).not.toHaveBeenCalled();

      await openTemplate(user);

      expect(retrieveScheduler).toHaveBeenCalledWith("acme", "sched-1");
      expect(screen.getByLabelText("Name")).toHaveValue("Security audit");
      expect(screen.getByLabelText("Description (optional)")).toHaveValue("Weekly sweep");
      expect(screen.getByLabelText("Prompt")).toHaveValue("Look for security issues");
      expect(screen.getByRole("button", { name: TEMPLATE_BUTTON })).toHaveAttribute("aria-expanded", "true");
      // Locked exactly like the workspace "Edit scheduler" form.
      expect(screen.getByLabelText("Slug")).toHaveValue("security-audit");
      expect(screen.getByLabelText("Slug")).toBeDisabled();
    });

    it("shows the shared-template notice with the active install count", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);

      const notice = screen.getByRole("note");
      expect(notice).toHaveTextContent("Changes apply to every project this scheduler is installed on");
      expect(notice).toHaveTextContent('{"count":3}');
    });

    it("collapses without asking when nothing was changed", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);

      await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));

      expect(screen.queryByLabelText("Prompt")).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("asks before discarding template edits, and keeps them on cancel", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "Edited prompt");

      await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));
      expect(screen.getByRole("alert")).toHaveTextContent("Discard your unsaved scheduler template changes?");
      expect(screen.getByLabelText("Prompt")).toHaveValue("Edited prompt");

      await user.click(screen.getByRole("button", { name: "Keep editing" }));
      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.getByLabelText("Prompt")).toHaveValue("Edited prompt");
    });

    it("discards template edits on confirm: reopening shows the saved values and Save sends no template update", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "Edited prompt");

      await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));
      await user.click(screen.getByRole("button", { name: "Discard changes" }));
      expect(screen.queryByLabelText("Prompt")).toBeNull();

      await openTemplate(user);
      expect(screen.getByLabelText("Prompt")).toHaveValue("Look for security issues");

      await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(updateBinding).toHaveBeenCalled());
      expect(updateScheduler).not.toHaveBeenCalled();
    });

    it("stays collapsed and reports it when the template cannot be loaded", async () => {
      const user = userEvent.setup();
      retrieveScheduler.mockRejectedValue({ error: "Nope" });
      renderModal();

      await user.click(screen.getByRole("button", { name: TEMPLATE_BUTTON }));

      await waitFor(() => expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ message: "Nope" })));
      expect(screen.queryByLabelText("Prompt")).toBeNull();
    });
  });

  describe("single save", () => {
    it("saves the prompt and the recurrence with one Save", async () => {
      const user = userEvent.setup();
      const { onUpdated, onClose } = renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "New prompt");
      await retype(user, "Recurrence (RRULE)", "FREQ=DAILY");

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(updateBinding).toHaveBeenCalledTimes(1));
      // Only the changed template field is sent — never the slug.
      expect(updateScheduler).toHaveBeenCalledTimes(1);
      expect(updateScheduler).toHaveBeenCalledWith("acme", "sched-1", { prompt: "New prompt" });
      expect(updateBinding).toHaveBeenCalledWith(
        "acme",
        "proj-1",
        "bind-1",
        expect.objectContaining({ rrule: "FREQ=DAILY" })
      );
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "SUCCESS", title: "Install and scheduler template updated" })
      );
      expect(onUpdated).toHaveBeenCalledWith(expect.objectContaining({ rrule: "FREQ=DAILY" }));
      expect(onClose).toHaveBeenCalled();
    });

    it("refreshes every scheduler cache of the workspace after a template save", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);
      await retype(user, "Name", "Renamed");
      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(mutateCache).toHaveBeenCalledTimes(1));
      const matches = mutateCache.mock.calls[0][0] as (key: unknown) => boolean;
      expect(matches(["schedulers", "acme"])).toBe(true);
      expect(matches(["scheduler-bindings", "acme", "proj-1"])).toBe(true);
      expect(matches(["scheduler-binding-detail", "acme", "proj-1", "bind-1"])).toBe(true);
      expect(matches(["scheduler-occurrences", "acme", "proj-1", "a", "b"])).toBe(true);
      expect(matches(["schedulers", "other-workspace"])).toBe(false);
      expect(matches(["pods", "acme"])).toBe(false);
      expect(matches("schedulers")).toBe(false);
    });

    it("sends no template update when the section was never opened", async () => {
      const user = userEvent.setup();
      const { onClose } = renderModal();

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(updateBinding).toHaveBeenCalledTimes(1));
      expect(updateScheduler).not.toHaveBeenCalled();
      expect(retrieveScheduler).not.toHaveBeenCalled();
      expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "SUCCESS", title: "Install updated" }));
      expect(onClose).toHaveBeenCalled();
    });

    it("sends no template update when the section is open but unchanged", async () => {
      const user = userEvent.setup();
      const { onClose } = renderModal();
      await openTemplate(user);
      // Typing a value back to what it was is not a change.
      await retype(user, "Prompt", "Look for security issues");

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(updateBinding).toHaveBeenCalledTimes(1));
      expect(updateScheduler).not.toHaveBeenCalled();
      expect(onClose).toHaveBeenCalled();
    });

    it("blocks the whole save while an open template field is invalid", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "");

      await user.click(screen.getByRole("button", { name: "Save" }));

      expect(await screen.findByText("Prompt is required.")).toBeInTheDocument();
      expect(updateScheduler).not.toHaveBeenCalled();
      expect(updateBinding).not.toHaveBeenCalled();
    });

    it("accepts an empty description", async () => {
      const user = userEvent.setup();
      renderModal();
      await openTemplate(user);
      await retype(user, "Description (optional)", "");

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(updateScheduler).toHaveBeenCalledWith("acme", "sched-1", { description: "" }));
    });
  });

  describe("permission", () => {
    it("hides the template button from a project admin who is not a workspace admin; the install still saves", async () => {
      const user = userEvent.setup();
      permissions.workspaceAdmin = false;
      const { onUpdated, onClose } = renderModal();

      expect(screen.queryByRole("button", { name: TEMPLATE_BUTTON })).toBeNull();

      await retype(user, "Recurrence (RRULE)", "FREQ=DAILY");
      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() =>
        expect(updateBinding).toHaveBeenCalledWith(
          "acme",
          "proj-1",
          "bind-1",
          expect.objectContaining({ rrule: "FREQ=DAILY" })
        )
      );
      expect(retrieveScheduler).not.toHaveBeenCalled();
      expect(updateScheduler).not.toHaveBeenCalled();
      expect(onUpdated).toHaveBeenCalled();
      expect(onClose).toHaveBeenCalled();
    });
  });

  describe("partial failure", () => {
    it("install saved, template failed: says so, stays open, and puts the error under its field", async () => {
      const user = userEvent.setup();
      updateScheduler.mockRejectedValue({ name: ["A scheduler with this name already exists."] });
      const { onUpdated, onClose } = renderModal();
      await openTemplate(user);
      await retype(user, "Name", "Duplicate");
      await retype(user, "Recurrence (RRULE)", "FREQ=DAILY");

      await user.click(screen.getByRole("button", { name: "Save" }));

      expect(await screen.findByText("A scheduler with this name already exists.")).toBeInTheDocument();
      expect(updateBinding).toHaveBeenCalledTimes(1);
      const toast = setToast.mock.calls[0][0] as { type: string; title: string; message: string };
      expect(toast.type).toBe("ERROR");
      expect(toast.title).toBe("Scheduler template not saved");
      expect(toast.message).toContain("The install settings were saved. The scheduler template was not saved");
      expect(toast.message).toContain("A scheduler with this name already exists.");
      // Still open, with the failed part still editable and its edit intact.
      expect(onClose).not.toHaveBeenCalled();
      expect(onUpdated).not.toHaveBeenCalled();
      expect(screen.getByLabelText("Name")).toHaveValue("Duplicate");
      expect(screen.getByLabelText("Name")).not.toBeDisabled();
      // The saved install shows up behind the dialog without closing it.
      expect(mutateCache).toHaveBeenCalledTimes(1);

      // Fixing the template and saving again finishes the job.
      updateScheduler.mockResolvedValue({ ...TEMPLATE, name: "Unique" });
      await retype(user, "Name", "Unique");
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(onClose).toHaveBeenCalled());
      expect(updateScheduler).toHaveBeenLastCalledWith("acme", "sched-1", { name: "Unique" });
    });

    it("template saved, install failed: says so, stays open, and does not resend the template", async () => {
      const user = userEvent.setup();
      updateBinding.mockRejectedValue({ rrule: ["Invalid RRULE."] });
      const { onUpdated, onClose } = renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "New prompt");
      await retype(user, "Recurrence (RRULE)", "FREQ=NOPE");

      await user.click(screen.getByRole("button", { name: "Save" }));

      expect(await screen.findByText("Invalid RRULE.")).toBeInTheDocument();
      expect(updateScheduler).toHaveBeenCalledTimes(1);
      const toast = setToast.mock.calls[0][0] as { type: string; title: string; message: string };
      expect(toast.type).toBe("ERROR");
      expect(toast.title).toBe("Install settings not saved");
      expect(toast.message).toContain("The scheduler template was saved. The install settings were not saved");
      expect(toast.message).toContain("Invalid RRULE.");
      expect(onClose).not.toHaveBeenCalled();
      expect(onUpdated).not.toHaveBeenCalled();
      expect(screen.getByLabelText("Recurrence (RRULE)")).toHaveValue("FREQ=NOPE");
      expect(mutateCache).toHaveBeenCalledTimes(1);

      // The template is already stored: the retry only sends the install.
      updateBinding.mockResolvedValue({ ...BINDING, rrule: "FREQ=DAILY" });
      await retype(user, "Recurrence (RRULE)", "FREQ=DAILY");
      await user.click(screen.getByRole("button", { name: "Save" }));
      await waitFor(() => expect(onClose).toHaveBeenCalled());
      expect(updateScheduler).toHaveBeenCalledTimes(1);
      expect(updateBinding).toHaveBeenCalledTimes(2);
    });

    it("both failed: reports that nothing was saved", async () => {
      const user = userEvent.setup();
      updateScheduler.mockRejectedValue({ error: "Template boom" });
      updateBinding.mockRejectedValue({ error: "Install boom" });
      const { onClose } = renderModal();
      await openTemplate(user);
      await retype(user, "Prompt", "New prompt");

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() =>
        expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ title: "Nothing was saved" }))
      );
      const toast = setToast.mock.calls[0][0] as { message: string };
      expect(toast.message).toContain("Template boom");
      expect(toast.message).toContain("Install boom");
      expect(mutateCache).not.toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();
    });

    it("install failed with the template untouched: plain error, dialog stays open", async () => {
      const user = userEvent.setup();
      updateBinding.mockRejectedValue({ error: "Install boom" });
      const { onClose } = renderModal();

      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() =>
        expect(setToast).toHaveBeenCalledWith({
          type: "ERROR",
          title: "Something went wrong",
          message: "Install boom",
        })
      );
      expect(onClose).not.toHaveBeenCalled();
    });
  });

  it("does not wipe in-progress edits when the caller revalidates the binding while the dialog is open", async () => {
    const user = userEvent.setup();
    const { rerender, onClose, onUpdated } = renderModal();
    await openTemplate(user);
    await retype(user, "Prompt", "Edited prompt");

    rerender(
      <EditSchedulerBindingModal
        isOpen
        onClose={onClose}
        workspaceSlug="acme"
        projectId="proj-1"
        binding={{ ...BINDING }}
        onUpdated={onUpdated}
      />
    );

    expect(screen.getByLabelText("Prompt")).toHaveValue("Edited prompt");
  });
});
