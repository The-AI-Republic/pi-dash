// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  params: {} as { ws?: string | undefined; projectId?: string | undefined; issueKey?: string | undefined },
  me: { data: { id: "u1" } as { id: string } | undefined },
  desktop: true,
  focusListeners: [] as Array<(focused: boolean) => void>,
  unsubscribe: vi.fn(),
  projectsFn: vi.fn(),
  getProject: vi.fn(),
  resume: vi.fn(),
  connect: vi.fn(),
  refresh: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  useParams: () => harness.params,
}));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  const desktopPlatform = {
    kind: "desktop",
    agentRuntime: {},
    onFocusChange: (callback: (focused: boolean) => void) => {
      harness.focusListeners.push(callback);
      return harness.unsubscribe;
    },
  };
  return {
    ...original,
    get platform() {
      return harness.desktop ? desktopPlatform : original.platform;
    },
  };
});

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => harness.me };
});

vi.mock("../../projects/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../projects/index.js")>();
  return {
    ...original,
    projectsQueryOptions: (_client: unknown, ws: string) => ({
      queryKey: ["projects", ws],
      queryFn: harness.projectsFn,
    }),
  };
});

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getProject: (...args: Array<unknown>) => harness.getProject(...args) };
});

vi.mock("./service.js", () => ({
  MANAGED_RUNNER_KIND: "managed_runner",
  resumeRuntime: (...args: Array<unknown>) => harness.resume(...args),
  connectProject: (...args: Array<unknown>) => harness.connect(...args),
  refreshRuntime: (...args: Array<unknown>) => harness.refresh(...args),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  AVAILABILITY_POLL_INTERVAL_MS,
  matchProjectIdentifier,
  RUNTIME_CONNECTED_TEXT,
  RUNTIME_PREPARING_TEXT,
  RUNTIME_WAITING_TEXT,
  RuntimeSupervisor,
  selectRuntimeNotice,
} from "./Supervisor.js";

let queryClient: QueryClient;

function waitingProject(): unknown {
  return { identifier: "ACME", agent_executor_options: [] };
}

function readyProject(): unknown {
  return {
    identifier: "ACME",
    agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  harness.params = { ws: "acme", projectId: "p1" };
  harness.me = { data: { id: "u1" } };
  harness.desktop = true;
  harness.focusListeners = [];
  harness.projectsFn.mockResolvedValue([]);
  harness.getProject.mockResolvedValue(waitingProject());
  harness.resume.mockClear();
  harness.connect.mockResolvedValue(undefined);
  harness.refresh.mockResolvedValue(undefined);
  harness.unsubscribe.mockClear();
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  createAppClient("https://api.test");
});

afterEach(() => {
  resetAppClient();
  vi.useRealTimers();
});

function renderSupervisor(props: { registerChatTransport?: () => void } = {}): void {
  render(
    <QueryClientProvider client={queryClient}>
      <RuntimeSupervisor {...props} />
    </QueryClientProvider>
  );
}

function focusApp(focused: boolean): void {
  for (const listener of harness.focusListeners) listener(focused);
}

describe("selectRuntimeNotice", () => {
  const base = { connectError: null, availabilityError: null, enrolled: false, available: false, refreshError: null };

  it("prefers the enrollment failure", () => {
    expect(selectRuntimeNotice({ ...base, connectError: "nope" })).toBe("nope");
  });

  it("prefers the poll failure over the phase", () => {
    expect(selectRuntimeNotice({ ...base, enrolled: true, availabilityError: "down" })).toBe("down");
  });

  it("shows preparing before enrollment", () => {
    expect(selectRuntimeNotice(base)).toBe(RUNTIME_PREPARING_TEXT);
  });

  it("shows waiting until the first heartbeat", () => {
    expect(selectRuntimeNotice({ ...base, enrolled: true })).toBe(RUNTIME_WAITING_TEXT);
  });

  it("shows runs-locally once available", () => {
    expect(selectRuntimeNotice({ ...base, enrolled: true, available: true })).toBe(RUNTIME_CONNECTED_TEXT);
  });

  it("surfaces a refresh failure once connected", () => {
    expect(selectRuntimeNotice({ ...base, enrolled: true, available: true, refreshError: "stale" })).toBe("stale");
  });
});

describe("matchProjectIdentifier", () => {
  const rows = [
    { id: "p-other", identifier: "OTHER" },
    { id: "p-acme", identifier: "ACME" },
  ] as never;

  it("matches the key prefix against the collection", () => {
    expect(matchProjectIdentifier(rows, "ACME-42")).toBe("p-acme");
  });

  it("resolves nothing without rows, without a key, or without a match", () => {
    expect(matchProjectIdentifier(undefined, "ACME-42")).toBeUndefined();
    expect(matchProjectIdentifier(rows, undefined)).toBeUndefined();
    expect(matchProjectIdentifier(rows, "NOPE-1")).toBeUndefined();
    expect(matchProjectIdentifier([], "ACME-42")).toBeUndefined();
  });
});

describe("RuntimeSupervisor", () => {
  it("renders nothing on web and runs no step", () => {
    harness.desktop = false;
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor registerChatTransport={vi.fn()} />
      </QueryClientProvider>
    );
    expect(container).toBeEmptyDOMElement();
    expect(harness.resume).not.toHaveBeenCalled();
    expect(harness.connect).not.toHaveBeenCalled();
    expect(harness.refresh).not.toHaveBeenCalled();
  });

  it("enrolls the project and waits for the first heartbeat", async () => {
    let release!: () => void;
    harness.connect.mockReturnValue(new Promise<void>((resolve) => void (release = resolve)));
    renderSupervisor();
    expect(await screen.findByTestId("runtime-notice")).toHaveTextContent(RUNTIME_PREPARING_TEXT);
    expect(harness.resume).toHaveBeenCalledWith("u1", expect.objectContaining({}));
    expect(harness.getProject).not.toHaveBeenCalled();
    await act(async () => {
      release();
    });
    await screen.findByText(RUNTIME_WAITING_TEXT);
    expect(harness.connect).toHaveBeenCalledWith(expect.objectContaining({ workspaceSlug: "acme", projectId: "p1" }));
    expect(harness.getProject).toHaveBeenCalled();
  });

  it("reports runs-locally once the server sees the daemon", async () => {
    harness.getProject.mockResolvedValue(readyProject());
    renderSupervisor();
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
  });

  it("registers the chat transport once the runtime resumes", async () => {
    const register = vi.fn();
    renderSupervisor({ registerChatTransport: register });
    await screen.findByTestId("runtime-notice");
    expect(register).toHaveBeenCalledTimes(1);
  });

  it("shows enrollment failures honestly", async () => {
    harness.connect.mockRejectedValue(new Error("daemon refused"));
    renderSupervisor();
    await screen.findByText("daemon refused");
  });

  it("clears a transient enrollment failure when focus reconnects", async () => {
    harness.connect.mockRejectedValueOnce(new Error("daemon refused")).mockResolvedValue(undefined);
    renderSupervisor();
    await screen.findByText("daemon refused");
    act(() => focusApp(true));
    await screen.findByText(RUNTIME_WAITING_TEXT);
    expect(harness.connect).toHaveBeenCalledTimes(2);
  });

  it("shows non-error enrollment failures as text", async () => {
    harness.connect.mockRejectedValue("boom");
    renderSupervisor();
    await screen.findByText("boom");
  });

  it("shows poll failures without stranding the poll", async () => {
    harness.getProject.mockRejectedValue(new Error("project read failed"));
    renderSupervisor();
    await screen.findByText("project read failed");
  });

  it("resumes polling after a failed availability re-check with stale available data", async () => {
    harness.getProject.mockResolvedValue(readyProject());
    renderSupervisor();
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
    vi.useFakeTimers();
    try {
      harness.getProject.mockRejectedValueOnce(new Error("re-check failed"));
      act(() => focusApp(true));
      await act(async () => undefined);
      await act(async () => {
        vi.advanceTimersByTime(0);
      });
      expect(screen.getByText("re-check failed")).toBeVisible();

      harness.getProject.mockResolvedValue(waitingProject());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(AVAILABILITY_POLL_INTERVAL_MS * 4);
      });
      expect(screen.getByText(RUNTIME_WAITING_TEXT)).toBeVisible();
      expect(harness.getProject.mock.calls.length).toBeGreaterThanOrEqual(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-polls availability while waiting", async () => {
    vi.useFakeTimers();
    try {
      renderSupervisor();
      await act(async () => undefined);
      expect(harness.getProject.mock.calls.length).toBeGreaterThan(0);
      harness.getProject.mockClear();
      await act(async () => {
        vi.advanceTimersByTime(20_000);
      });
      expect(harness.getProject.mock.calls.length).toBeGreaterThanOrEqual(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops the poll once available", async () => {
    harness.getProject.mockResolvedValue(readyProject());
    renderSupervisor();
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
    vi.useFakeTimers();
    try {
      harness.getProject.mockClear();
      await act(async () => {
        vi.advanceTimersByTime(30_000);
      });
      expect(harness.getProject).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("restarts a stopped wait on focus when the agent went away", async () => {
    harness.getProject.mockResolvedValue(readyProject());
    renderSupervisor();
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
    expect(harness.getProject).toHaveBeenCalledTimes(1);
    harness.getProject.mockResolvedValue(waitingProject());
    act(() => focusApp(true));
    await screen.findByText(RUNTIME_WAITING_TEXT);
    expect(harness.connect).toHaveBeenCalledTimes(2);
    expect(harness.getProject).toHaveBeenCalledTimes(2);
  });

  it("ignores focus loss", async () => {
    renderSupervisor();
    await screen.findByTestId("runtime-notice");
    act(() => focusApp(false));
    expect(harness.connect).toHaveBeenCalledTimes(1);
    expect(harness.refresh).not.toHaveBeenCalled();
  });

  it("refreshes on the minute cadence", async () => {
    vi.useFakeTimers();
    try {
      renderSupervisor();
      await act(async () => undefined);
      expect(harness.refresh).not.toHaveBeenCalled();
      await act(async () => {
        vi.advanceTimersByTime(60_000);
      });
      expect(harness.refresh).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("surfaces refresh failures once connected and clears them on recovery", async () => {
    harness.getProject.mockResolvedValue(readyProject());
    harness.refresh.mockRejectedValueOnce(new Error("setup rewrite failed"));
    renderSupervisor();
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
    act(() => focusApp(true));
    await screen.findByText("setup rewrite failed");
    act(() => focusApp(true));
    await screen.findByText(RUNTIME_CONNECTED_TEXT);
  });

  it("ignores a refresh that settles after unmount", async () => {
    let release!: () => void;
    harness.refresh.mockReturnValue(new Promise<void>((resolve) => void (release = resolve)));
    const rendered = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    await act(async () => undefined);
    act(() => focusApp(true));
    rendered.unmount();
    await act(async () => {
      release();
    });
  });

  it("ignores a refresh failure that settles after unmount", async () => {
    let reject!: (error: unknown) => void;
    harness.refresh.mockReturnValue(new Promise<void>((_resolve, rejectPromise) => void (reject = rejectPromise)));
    const rendered = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    await act(async () => undefined);
    act(() => focusApp(true));
    rendered.unmount();
    await act(async () => {
      reject(new Error("late refresh"));
    });
  });

  it("ignores an enrollment that settles after unmount", async () => {
    let release!: () => void;
    harness.connect.mockReturnValue(new Promise<void>((resolve) => void (release = resolve)));
    const rendered = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    await act(async () => undefined);
    rendered.unmount();
    await act(async () => {
      release();
    });
  });

  it("ignores an enrollment failure after unmount", async () => {
    let reject!: (error: unknown) => void;
    harness.connect.mockReturnValue(new Promise<void>((_resolve, rejectPromise) => void (reject = rejectPromise)));
    const rendered = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    await act(async () => undefined);
    rendered.unmount();
    await act(async () => {
      reject(new Error("late"));
    });
  });

  it("cleans up timers and focus listeners on unmount", async () => {
    vi.useFakeTimers();
    try {
      const rendered = render(
        <QueryClientProvider client={queryClient}>
          <RuntimeSupervisor />
        </QueryClientProvider>
      );
      await act(async () => undefined);
      rendered.unmount();
      expect(harness.unsubscribe).toHaveBeenCalledTimes(2);
      await act(async () => {
        vi.advanceTimersByTime(60_000);
      });
      expect(harness.refresh).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stays quiet while signed out", () => {
    harness.me = { data: undefined };
    renderSupervisor();
    expect(screen.queryByTestId("runtime-notice")).toBeNull();
    expect(harness.resume).not.toHaveBeenCalled();
    expect(harness.connect).not.toHaveBeenCalled();
  });

  it("stays quiet without a project", () => {
    harness.params = { ws: "acme" };
    renderSupervisor();
    expect(screen.queryByTestId("runtime-notice")).toBeNull();
    expect(harness.projectsFn).not.toHaveBeenCalled();
    expect(harness.connect).not.toHaveBeenCalled();
  });

  it("resolves direct work-item links to their project", async () => {
    harness.params = { ws: "acme", issueKey: "ACME-42" };
    harness.projectsFn.mockResolvedValue([
      { id: "p-other", identifier: "OTHER" },
      { id: "p-browse", identifier: "ACME" },
    ]);
    renderSupervisor();
    await screen.findByText(RUNTIME_WAITING_TEXT);
    expect(harness.projectsFn).toHaveBeenCalled();
    expect(harness.connect).toHaveBeenCalledWith(expect.objectContaining({ projectId: "p-browse" }));
  });

  it("stays quiet when the work-item link resolves nowhere", async () => {
    harness.params = { ws: "acme", issueKey: "ACME-42" };
    harness.projectsFn.mockResolvedValue([{ id: "p-other", identifier: "OTHER" }]);
    renderSupervisor();
    await act(async () => undefined);
    expect(screen.queryByTestId("runtime-notice")).toBeNull();
    expect(harness.connect).not.toHaveBeenCalled();
  });

  it("stays quiet outside the workspace", () => {
    harness.params = {};
    renderSupervisor();
    expect(screen.queryByTestId("runtime-notice")).toBeNull();
    expect(harness.resume).toHaveBeenCalled();
    expect(harness.connect).not.toHaveBeenCalled();
  });

  it("re-enrolls when the project changes", async () => {
    const rendered = render(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    await screen.findByText(RUNTIME_WAITING_TEXT);
    harness.params = { ws: "acme", projectId: "p2" };
    rendered.rerender(
      <QueryClientProvider client={queryClient}>
        <RuntimeSupervisor />
      </QueryClientProvider>
    );
    expect(await screen.findByText(RUNTIME_PREPARING_TEXT)).toBeVisible();
    expect(harness.connect).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: "p2" }));
  });
});
