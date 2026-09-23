/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { apiBaseUrl, listPods, listProjects, setToast, listDevMachines } = vi.hoisted(() => ({
  apiBaseUrl: { value: "http://localhost:8000" },
  listPods: vi.fn(),
  listProjects: vi.fn(),
  setToast: vi.fn(),
  listDevMachines: vi.fn(),
}));

vi.mock("@pi-dash/constants", () => ({
  get API_BASE_URL() {
    return apiBaseUrl.value;
  },
}));

vi.mock("@pi-dash/services", () => ({
  PodService: class {
    list = listPods;
  },
  RunnerService: class {
    listDevMachines = listDevMachines;
    createRunnerOnMachine = vi.fn();
    getCreateRunnerOnMachineStatus = vi.fn();
  },
}));

vi.mock("@/services/project", () => ({
  ProjectService: class {
    getProjectsLite = listProjects;
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

import { AddRunnerModal } from "@/components/runners/add-runner-modal";

const PROJECTS = [{ id: "project-1", identifier: "BROWSERX", name: "BrowserX" }];
const PODS = [
  {
    id: "pod-1",
    name: "pod-a",
    description: "",
    is_default: false,
    workspace: "workspace-1",
    project: "project-1",
    project_identifier: "BROWSERX",
    created_by: null,
    runner_count: 0,
    created_at: "2026-05-23T00:00:00Z",
    updated_at: "2026-05-23T00:00:00Z",
  },
];

describe("AddRunnerModal — Muse Code agent", () => {
  beforeEach(() => {
    apiBaseUrl.value = "http://localhost:8000";
    listPods.mockReset().mockResolvedValue(PODS);
    listProjects.mockReset().mockResolvedValue(PROJECTS);
    listDevMachines.mockReset().mockResolvedValue([]);
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal(workspaceId = "workspace-muse") {
    const onClose = vi.fn();
    const utils = render(<AddRunnerModal isOpen onClose={onClose} workspaceId={workspaceId} workspaceSlug="acme" />);
    return { ...utils, onClose };
  }

  it("lists 'Muse Code' as a selectable agent in the dropdown", async () => {
    renderModal();
    await screen.findByRole("option", { name: "BrowserX" });

    // selects: [0]=dev machine, [1]=project, [2]=pod, [3]=agent, [4]=model
    const agentSelect = screen.getAllByTestId("select")[3];
    expect(within(agentSelect).getByRole("option", { name: "Muse Code" })).toBeInTheDocument();
  });

  it("generates a `--agent muse-code` runner-add command", async () => {
    const user = userEvent.setup();
    renderModal();
    await screen.findByRole("option", { name: "BrowserX" });

    const selects = screen.getAllByTestId("select");
    await user.selectOptions(selects[1], "BROWSERX");
    await user.selectOptions(selects[2], "pod-a");
    await user.type(screen.getByPlaceholderText("my-laptop-runner"), "muse-local");
    await user.type(screen.getByPlaceholderText("local dev machine project working dir"), "/home/rich/dev/browserx");
    await user.selectOptions(selects[3], "muse-code");
    await user.click(screen.getByRole("button", { name: "Generate Runner" }));

    const command = await screen.findByText(
      (_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre"
    );
    expect(command.textContent).toContain("pidash runner add");
    expect(command.textContent).toContain("--agent muse-code");
    expect(command.textContent).toContain("--name muse-local");
  });
});
