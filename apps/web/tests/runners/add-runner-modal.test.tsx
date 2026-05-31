/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { apiBaseUrl, listPods, listProjects, setToast } = vi.hoisted(() => ({
  apiBaseUrl: { value: "http://localhost:8000" },
  listPods: vi.fn(),
  listProjects: vi.fn(),
  setToast: vi.fn(),
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

describe("AddRunnerModal", () => {
  beforeEach(() => {
    apiBaseUrl.value = "http://localhost:8000";
    listPods.mockReset().mockResolvedValue(PODS);
    listProjects.mockReset().mockResolvedValue(PROJECTS);
    setToast.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function renderModal() {
    const onClose = vi.fn();
    const utils = render(<AddRunnerModal isOpen onClose={onClose} workspaceId="workspace-1" workspaceSlug="acme" />);
    return { ...utils, onClose };
  }

  it("generates the runner-add command for the selected project", async () => {
    const user = userEvent.setup();
    const workingDir = "/home/rich/dev/airepublic/open_source/s6/browserx";
    const runnerName = "browserx-local";
    renderModal();

    await screen.findByRole("option", { name: "BrowserX" });

    const selects = screen.getAllByTestId("select");
    await user.selectOptions(selects[0], "BROWSERX");
    await user.selectOptions(selects[1], "pod-a");
    await user.type(screen.getByPlaceholderText("runners.add_modal.name_placeholder"), runnerName);
    await user.type(screen.getByPlaceholderText("runners.add_modal.working_dir_placeholder"), workingDir);
    await user.selectOptions(selects[2], "codex");
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));

    const command = await screen.findByText(
      (_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre"
    );
    expect(command.textContent).not.toContain("pidash auth login");
    expect(command.textContent).toContain("pidash runner add");
    expect(command.textContent).toContain("--url http://localhost:8000");
    expect(command.textContent).toContain("--workspace acme");
    expect(command.textContent).toContain("--project BROWSERX");
    expect(command.textContent).toContain("--pod pod-a");
    expect(command.textContent).toContain(`--name ${runnerName}`);
    expect(command.textContent).toContain(`--working-dir ${workingDir}`);
    expect(command.textContent).toContain("--agent codex");
    expect(command.textContent).not.toContain("pidash connect");
    expect(command.textContent).not.toContain("--token");
  });

  it("lets the user go back and edit the form after generating a command", async () => {
    const user = userEvent.setup();
    const runnerName = "browserx-local";
    renderModal();

    await screen.findByRole("option", { name: "BrowserX" });

    await user.selectOptions(screen.getAllByTestId("select")[0], "BROWSERX");
    await user.type(screen.getByPlaceholderText("runners.add_modal.name_placeholder"), runnerName);
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));

    await screen.findByText((_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre");
    await user.click(screen.getByRole("button", { name: "runners.add_modal.back" }));

    expect(screen.getByPlaceholderText("runners.add_modal.name_placeholder")).toHaveValue(runnerName);
  });

  it("blocks invalid runner names before generating a command", async () => {
    const user = userEvent.setup();
    renderModal();

    await screen.findByRole("option", { name: "BrowserX" });

    await user.selectOptions(screen.getAllByTestId("select")[0], "BROWSERX");
    await user.type(screen.getByPlaceholderText("runners.add_modal.name_placeholder"), "test runner");
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));

    const error = await screen.findByText("runners.add_modal.errors.name_invalid");
    expect(error).toBeInTheDocument();
    expect(error).toHaveClass("text-danger-primary");
    expect(
      screen.queryByText((_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre")
    ).not.toBeInTheDocument();
  });

  it("renders PowerShell-safe quoting for Windows users", async () => {
    const user = userEvent.setup();
    renderModal();

    await screen.findByRole("option", { name: "BrowserX" });

    const selects = screen.getAllByTestId("select");
    await user.selectOptions(selects[0], "BROWSERX");
    await user.type(screen.getByPlaceholderText("runners.add_modal.name_placeholder"), "rich.runner");
    await user.type(
      screen.getByPlaceholderText("runners.add_modal.working_dir_placeholder"),
      String.raw`C:\\Users\\rich\\My Project`
    );
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));
    await user.click(screen.getByRole("button", { name: "runners.add_modal.shell_powershell" }));

    const command = await screen.findByText(
      (_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre"
    );
    expect(command.textContent).toContain("--name rich.runner");
    expect(command.textContent).toContain(String.raw`--working-dir 'C:\\Users\\rich\\My Project'`);
    expect(command.textContent).not.toContain("'\\''");
  });

  it("warns when the command URL falls back to the browser origin", async () => {
    apiBaseUrl.value = "";
    const user = userEvent.setup();
    renderModal();

    await screen.findByRole("option", { name: "BrowserX" });
    await user.selectOptions(screen.getAllByTestId("select")[0], "BROWSERX");
    await user.click(screen.getByRole("button", { name: "runners.add_modal.submit" }));

    const command = await screen.findByText(
      (_content: string, node: Element | null) => node?.tagName.toLowerCase() === "pre"
    );
    expect(command.textContent).toContain(`--url ${window.location.origin}`);
    expect(screen.getByText("runners.add_modal.cloud_url_origin_warning")).toBeInTheDocument();
  });
});
