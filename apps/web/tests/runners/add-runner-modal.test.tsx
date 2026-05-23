/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { createRunnerInvite, listPods, listProjects, setToast } = vi.hoisted(() => ({
  createRunnerInvite: vi.fn(),
  listPods: vi.fn(),
  listProjects: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/constants", () => ({
  API_BASE_URL: "http://localhost:8000",
}));

vi.mock("@pi-dash/services", () => ({
  PodService: class {
    list = listPods;
  },
  RunnerService: class {
    createRunnerInvite = createRunnerInvite;
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

const INVITE = {
  runner_id: "runner-1",
  name: "runner-1",
  workspace_slug: "acme",
  project_identifier: "BROWSERX",
  pod_id: "pod-1",
  enrollment_token: "apd_test_token",
  enrollment_expires_at: "2026-05-23T00:30:00Z",
};

describe("AddRunnerModal", () => {
  beforeEach(() => {
    createRunnerInvite.mockReset().mockResolvedValue(INVITE);
    listPods.mockReset().mockResolvedValue(PODS);
    listProjects.mockReset().mockResolvedValue(PROJECTS);
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal() {
    const onClose = vi.fn();
    const onCreated = vi.fn();
    const utils = render(
      <AddRunnerModal isOpen onClose={onClose} workspaceId="workspace-1" workspaceSlug="acme" onCreated={onCreated} />
    );
    return { ...utils, onClose, onCreated };
  }

  it("includes working-dir when the submitted agent is codex", async () => {
    const user = userEvent.setup();
    const workingDir = "/home/rich/dev/airepublic/open_source/s6/browserx";
    const { onCreated } = renderModal();

    await screen.findByRole("option", { name: "BrowserX" });

    const selects = screen.getAllByTestId("select");
    await user.selectOptions(selects[0], "BROWSERX");
    await user.type(screen.getByPlaceholderText("runners.add_modal.working_dir_placeholder"), workingDir);
    await user.selectOptions(selects[2], "codex");
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));

    await waitFor(() => {
      expect(createRunnerInvite).toHaveBeenCalledWith({
        workspaceId: "workspace-1",
        projectIdentifier: "BROWSERX",
        podName: undefined,
        name: undefined,
      });
    });
    expect(onCreated).toHaveBeenCalled();

    const command = await screen.findByText(
      (_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre"
    );
    expect(command.textContent).toContain(`--working-dir ${workingDir}`);
    expect(command.textContent).toContain("--agent codex");
  });
});
