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
const { podUpdate, setToast } = vi.hoisted(() => ({
  podUpdate: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  PodService: class {
    update = podUpdate;
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

// Replace the heavy UI lib with plain DOM equivalents so the modal can be
// driven with userEvent without dragging in headlessui machinery. The
// ToggleSwitch stands in as a checkbox.
vi.mock("@pi-dash/ui", async () => {
  const { forwardRef: fwd } = await import("react");
  return {
    ModalCore: ({ isOpen, children }: { isOpen: boolean; children: React.ReactNode }) =>
      isOpen ? <div role="dialog">{children}</div> : null,
    EModalPosition: { CENTER: "CENTER" },
    EModalWidth: { XXL: "XXL" },
    Input: fwd<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(function Input(props, ref) {
      return <input ref={ref} {...props} />;
    }),
    ToggleSwitch: ({
      value,
      onChange,
      disabled,
      label,
    }: {
      value: boolean;
      onChange: (v: boolean) => void;
      disabled?: boolean;
      label?: string;
    }) => (
      <input
        type="checkbox"
        aria-label={label}
        checked={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
    ),
  };
});

import { EditPodModal } from "@/components/runners/edit-pod-modal";

const POD = {
  id: "pod-1",
  name: "WEB_beefy",
  description: "the spare laptop",
  is_default: false,
  workspace: "ws-1",
  project: "proj-1",
  project_identifier: "WEB",
  created_by: "user-1",
  runner_count: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

describe("EditPodModal", () => {
  beforeEach(() => {
    podUpdate.mockReset();
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal(overrides: Partial<React.ComponentProps<typeof EditPodModal>> = {}) {
    const onClose = vi.fn();
    const onUpdated = vi.fn();
    const utils = render(<EditPodModal isOpen pod={POD} onClose={onClose} onUpdated={onUpdated} {...overrides} />);
    return { ...utils, onClose, onUpdated };
  }

  it("renders nothing when isOpen=false", () => {
    render(<EditPodModal isOpen={false} pod={POD} onClose={vi.fn()} onUpdated={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("renders nothing when pod is null", () => {
    render(<EditPodModal isOpen pod={null} onClose={vi.fn()} onUpdated={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("prefills the name suffix (project prefix stripped) and description", () => {
    renderModal();
    expect((screen.getByPlaceholderText("beefy") as HTMLInputElement).value).toBe("beefy");
    expect((screen.getByPlaceholderText("Where this pod runs, what it's for, etc.") as HTMLInputElement).value).toBe(
      "the spare laptop"
    );
  });

  it("blocks submit when name is whitespace and shows name_required", async () => {
    const user = userEvent.setup();
    renderModal();
    await user.clear(screen.getByPlaceholderText("beefy"));
    await user.type(screen.getByPlaceholderText("beefy"), "   ");
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    expect(podUpdate).not.toHaveBeenCalled();
    expect(await screen.findByText("Name is required.")).toBeInTheDocument();
  });

  it("closes without calling update when nothing changed", async () => {
    const user = userEvent.setup();
    const { onClose, onUpdated } = renderModal();
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(podUpdate).not.toHaveBeenCalled();
    expect(onUpdated).not.toHaveBeenCalled();
  });

  it("sends only the changed name (trimmed) and closes on success", async () => {
    const user = userEvent.setup();
    podUpdate.mockResolvedValue({ ...POD, name: "WEB_chonky" });
    const { onClose, onUpdated } = renderModal();

    await user.clear(screen.getByPlaceholderText("beefy"));
    await user.type(screen.getByPlaceholderText("beefy"), "  chonky  ");
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => {
      expect(podUpdate).toHaveBeenCalledWith("pod-1", { name: "chonky" });
    });
    expect(onUpdated).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
    expect(setToast).not.toHaveBeenCalled();
  });

  it("sends is_default when the pod is promoted to default", async () => {
    const user = userEvent.setup();
    podUpdate.mockResolvedValue({ ...POD, is_default: true });
    renderModal();

    await user.click(screen.getByRole("checkbox", { name: "Project default" }));
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => {
      expect(podUpdate).toHaveBeenCalledWith("pod-1", { is_default: true });
    });
  });

  it("disables the default toggle for a pod that is already the default", () => {
    render(<EditPodModal isOpen pod={{ ...POD, is_default: true }} onClose={vi.fn()} onUpdated={vi.fn()} />);
    expect(screen.getByRole("checkbox", { name: "Project default" })).toBeDisabled();
  });

  it("shows a toast with the backend error and keeps the modal open on failure", async () => {
    const user = userEvent.setup();
    podUpdate.mockRejectedValue({ error: "name already exists" });
    const { onClose, onUpdated } = renderModal();

    await user.clear(screen.getByPlaceholderText("beefy"));
    await user.type(screen.getByPlaceholderText("beefy"), "chonky");
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => {
      expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "ERROR", message: "name already exists" }));
    });
    expect(onUpdated).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("falls back to the generic update_failed message when the error has no .error field", async () => {
    const user = userEvent.setup();
    podUpdate.mockRejectedValue(null);
    renderModal();

    await user.clear(screen.getByPlaceholderText("beefy"));
    await user.type(screen.getByPlaceholderText("beefy"), "chonky");
    await user.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => {
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "ERROR", message: "Could not update the pod." })
      );
    });
  });
});
