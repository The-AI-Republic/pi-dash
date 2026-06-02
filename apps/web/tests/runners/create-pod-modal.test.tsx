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
const { podCreate, projectsList, setToast } = vi.hoisted(() => ({
  podCreate: vi.fn(),
  projectsList: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  PodService: class {
    create = podCreate;
  },
}));

vi.mock("@/services/project", () => ({
  ProjectService: class {
    getProjectsLite = projectsList;
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
  type SelectProps = {
    value: string;
    onChange: (v: string) => void;
    children: React.ReactNode;
    disabled?: boolean;
  };
  // eslint-disable-next-line unicorn/consistent-function-scoping -- inside vi.mock factory; cannot hoist out
  const Sel = ({ value, onChange, children, disabled }: SelectProps) => (
    <select data-testid="select" value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
      <option value="" disabled>
        placeholder
      </option>
      {children}
    </select>
  );
  Sel.Option = ({ value, children }: { value: string; children: React.ReactNode }) => (
    <option value={value}>{children}</option>
  );
  return {
    ModalCore: ({ isOpen, children }: { isOpen: boolean; children: React.ReactNode }) =>
      isOpen ? <div role="dialog">{children}</div> : null,
    EModalPosition: { CENTER: "CENTER" },
    EModalWidth: { XXL: "XXL" },
    Input: fwd<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(function Input(props, ref) {
      return <input ref={ref} {...props} />;
    }),
    CustomSelect: Sel,
  };
});

import { CreatePodModal } from "@/components/runners/create-pod-modal";

const PROJECTS = [
  { id: "proj-1", identifier: "WEB", name: "Web app" },
  { id: "proj-2", identifier: "API", name: "API" },
];

describe("CreatePodModal", () => {
  beforeEach(() => {
    podCreate.mockReset();
    projectsList.mockReset().mockResolvedValue(PROJECTS);
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal(overrides: Partial<React.ComponentProps<typeof CreatePodModal>> = {}) {
    const onClose = vi.fn();
    const onCreated = vi.fn();
    const utils = render(
      <CreatePodModal isOpen onClose={onClose} workspaceSlug="acme" onCreated={onCreated} {...overrides} />
    );
    return { ...utils, onClose, onCreated };
  }

  it("renders nothing when isOpen=false", () => {
    render(<CreatePodModal isOpen={false} onClose={vi.fn()} workspaceSlug="acme" onCreated={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("loads projects via SWR when opened", async () => {
    renderModal();
    await waitFor(() => {
      expect(projectsList).toHaveBeenCalledWith("acme");
    });
    expect(await screen.findByRole("option", { name: "Web app" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "API" })).toBeInTheDocument();
  });

  it("blocks submit when project is unselected and shows the project_required error", async () => {
    const user = userEvent.setup();
    renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.click(screen.getByRole("button", { name: "Create pod" }));

    expect(podCreate).not.toHaveBeenCalled();
    expect(await screen.findByText("Pick a project.")).toBeInTheDocument();
  });

  it("blocks submit when name is empty / whitespace and shows name_required", async () => {
    const user = userEvent.setup();
    renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.selectOptions(screen.getByTestId("select"), "proj-1");
    // whitespace-only — RHF validate() trims and rejects
    await user.type(screen.getByPlaceholderText("beefy"), "   ");
    await user.click(screen.getByRole("button", { name: "Create pod" }));

    expect(podCreate).not.toHaveBeenCalled();
    expect(await screen.findByText("Name is required.")).toBeInTheDocument();
  });

  it("submits with trimmed name + project id and closes on success", async () => {
    const user = userEvent.setup();
    podCreate.mockResolvedValue({ id: "pod-1", name: "WEB_beefy" });
    const { onClose, onCreated } = renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.selectOptions(screen.getByTestId("select"), "proj-1");
    await user.type(screen.getByPlaceholderText("beefy"), "  beefy  ");
    await user.click(screen.getByRole("button", { name: "Create pod" }));

    await waitFor(() => {
      expect(podCreate).toHaveBeenCalledWith({
        project: "proj-1",
        name: "beefy",
        description: undefined,
      });
    });
    expect(onCreated).toHaveBeenCalledWith({ id: "pod-1", name: "WEB_beefy" });
    expect(onClose).toHaveBeenCalled();
    expect(setToast).not.toHaveBeenCalled();
  });

  it("passes description through when provided", async () => {
    const user = userEvent.setup();
    podCreate.mockResolvedValue({ id: "pod-1", name: "WEB_beefy" });
    renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.selectOptions(screen.getByTestId("select"), "proj-1");
    await user.type(screen.getByPlaceholderText("beefy"), "beefy");
    await user.type(
      screen.getByPlaceholderText("Where this pod runs, what it's for, etc."),
      "the spare laptop"
    );
    await user.click(screen.getByRole("button", { name: "Create pod" }));

    await waitFor(() => {
      expect(podCreate).toHaveBeenCalledWith({
        project: "proj-1",
        name: "beefy",
        description: "the spare laptop",
      });
    });
  });

  it("shows a toast with the backend error message on failure and keeps the modal open", async () => {
    const user = userEvent.setup();
    podCreate.mockRejectedValue({ error: "name already exists" });
    const { onClose, onCreated } = renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.selectOptions(screen.getByTestId("select"), "proj-1");
    await user.type(screen.getByPlaceholderText("beefy"), "beefy");
    await user.click(screen.getByRole("button", { name: "Create pod" }));

    await waitFor(() => {
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "ERROR",
          message: "name already exists",
        })
      );
    });
    expect(onCreated).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("falls back to the generic create_failed message when the error has no .error field", async () => {
    const user = userEvent.setup();
    podCreate.mockRejectedValue(null);
    renderModal();
    await screen.findByRole("option", { name: "Web app" });

    await user.selectOptions(screen.getByTestId("select"), "proj-1");
    await user.type(screen.getByPlaceholderText("beefy"), "beefy");
    await user.click(screen.getByRole("button", { name: "Create pod" }));

    await waitFor(() => {
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "ERROR",
          message: "Could not create the pod.",
        })
      );
    });
  });
});
