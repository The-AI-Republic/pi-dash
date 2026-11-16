/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { useSWR, mutateCache, allowPermissions, modalProps } = vi.hoisted(() => ({
  useSWR: vi.fn(),
  mutateCache: vi.fn(),
  allowPermissions: vi.fn(),
  modalProps: { current: null as null | Record<string, unknown> },
}));

vi.mock("swr", () => ({
  default: useSWR,
  useSWRConfig: () => ({ mutate: mutateCache }),
}));

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@pi-dash/constants", () => ({
  EUserPermissions: { ADMIN: 20 },
  EUserPermissionsLevel: { PROJECT: "PROJECT", WORKSPACE: "WORKSPACE" },
}));

vi.mock("@pi-dash/services", () => ({
  SchedulerService: class {
    listBindings = vi.fn();
    listSchedulers = vi.fn();
  },
}));

vi.mock("@pi-dash/ui", () => ({
  Button: ({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) => (
    <button type="button" onClick={onClick}>
      {children}
    </button>
  ),
}));

vi.mock("@/components/project/scheduler-bindings/new-scheduler-modal", () => ({
  NewSchedulerModal: (props: Record<string, unknown>) => {
    modalProps.current = props;
    return props.isOpen ? <div role="dialog">new-scheduler-modal</div> : null;
  },
}));

vi.mock("@/hooks/store/user", () => ({
  useUserPermissions: () => ({ allowPermissions }),
}));

import {
  NewSchedulerButton,
  isProjectSchedulerKey,
} from "../../core/components/project/scheduler-bindings/new-scheduler-button";

function renderButton() {
  return render(<NewSchedulerButton workspaceSlug="acme" projectId="proj-1" />);
}

describe("NewSchedulerButton (Schedulers tab bar)", () => {
  beforeEach(() => {
    useSWR.mockReset();
    mutateCache.mockReset();
    allowPermissions.mockReset();
    modalProps.current = null;
    useSWR.mockImplementation((key: unknown) => {
      const kind = Array.isArray(key) ? key[0] : null;
      if (kind === "scheduler-bindings") return { data: [], mutate: vi.fn() };
      if (kind === "schedulers") return { data: [], mutate: vi.fn() };
      return { data: undefined, mutate: vi.fn() };
    });
  });

  it("shows the button to a project admin and opens the New Scheduler modal", () => {
    allowPermissions.mockReturnValue(true);
    renderButton();

    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByText("New Scheduler"));
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("renders nothing for a project member and skips the fetches", () => {
    allowPermissions.mockReturnValue(false);
    const { container } = renderButton();

    expect(screen.queryByText("New Scheduler")).toBeNull();
    expect(container.innerHTML).toBe("");
    for (const call of useSWR.mock.calls) expect(call[0]).toBeNull();
  });

  it("refreshes the project's bindings and calendar occurrences after an install", () => {
    allowPermissions.mockReturnValue(true);
    renderButton();

    const props = modalProps.current;
    if (!props) throw new Error("NewSchedulerModal was not rendered");
    (props.onInstalled as () => void)();
    expect(mutateCache).toHaveBeenCalledTimes(1);
    const matcher = mutateCache.mock.calls[0][0] as (key: unknown) => boolean;
    expect(matcher(["scheduler-bindings", "acme", "proj-1"])).toBe(true);
    expect(matcher(["scheduler-occurrences", "acme", "proj-1", "2026-11-01", "2026-11-08"])).toBe(true);
    expect(matcher(["scheduler-bindings", "acme", "other"])).toBe(false);
    expect(matcher(["schedulers", "acme"])).toBe(false);
  });
});

describe("isProjectSchedulerKey", () => {
  it("rejects non-array keys", () => {
    expect(isProjectSchedulerKey("scheduler-bindings", "acme", "proj-1")).toBe(false);
  });
});
