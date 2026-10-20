// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { setSessionClient } from "../../core/session/queries.js";
import { MemberPicker } from "./MemberPicker.js";
import { pickerKeys } from "./reference.js";

const ME = "11111111-1111-4111-8111-111111111111";
const ANA = "22222222-2222-4222-8222-222222222222";
const ZOE = "33333333-3333-4333-8333-333333333333";
const MAX = "44444444-4444-4444-8444-444444444444";
const GUEST = "55555555-5555-4555-8555-555555555555";
const PROJECT = "66666666-6666-4666-8666-666666666666";

const ME_PAYLOAD = {
  id: ME,
  email: "parity@example.com",
  username: "parity",
  first_name: "Parity",
  last_name: "User",
  display_name: "Parity User",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function projectRow(userId: string, role = 15) {
  return {
    id: `${userId.slice(0, -1)}2`,
    role,
    member: userId,
    project: PROJECT,
    original_role: role,
    created_at: "2026-01-01T00:00:00.000Z",
  };
}

function directoryRow(
  userId: string,
  displayName: string,
  role = 15,
  isActive = true,
  avatarUrl: string | null = null
) {
  return {
    id: `${userId.slice(0, -1)}3`,
    member: {
      id: userId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: avatarUrl,
      display_name: displayName,
      is_bot: false,
    },
    role,
    is_active: isActive,
  };
}

const PROJECT_MEMBERS = [projectRow(ME), projectRow(ANA), projectRow(ZOE), projectRow(MAX), projectRow(GUEST, 5)];
const AVATAR = "https://cdn.test/avatars/ana.png";
const DIRECTORY = [
  directoryRow(ME, "Parity User"),
  directoryRow(ANA, "Ana", 15, true, AVATAR),
  directoryRow(ZOE, "Zoe", 15, false, "https://cdn.test/avatars/zoe.png"),
  directoryRow(MAX, "Max"),
  directoryRow(GUEST, "Parity Guest", 5),
];

interface Route {
  status: number;
  body: string;
  gate?: Promise<void>;
}

function harness(routes: (url: string) => Route, seedMe = true) {
  const calls: string[] = [];
  const transport = async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    const route = routes(url);
    if (route.gate !== undefined) await route.gate;
    return {
      status: route.status,
      url,
      headers: { get: () => "application/json" },
      text: async () => route.body,
    };
  };
  const client = createAppClient("https://api.test", transport as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seedMe) queryClient.setQueryData(["session", "me"], ME_PAYLOAD);
  return { queryClient, calls };
}

function memberRoutes() {
  return (url: string): Route => {
    if (url === "https://api.test/api/users/me/") return { status: 200, body: JSON.stringify(ME_PAYLOAD) };
    if (url.includes("/projects/") && url.endsWith("/members/")) {
      return { status: 200, body: JSON.stringify(PROJECT_MEMBERS) };
    }
    if (url.endsWith("/members/")) return { status: 200, body: JSON.stringify(DIRECTORY) };
    throw new Error(`unexpected url ${url}`);
  };
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

function setupPicker(
  queryClient: QueryClient,
  props: DistributiveOmit<React.ComponentProps<typeof MemberPicker>, "workspaceSlug" | "onChange" | "onOpenChange">
) {
  const onChange = vi.fn();
  const onOpenChange = vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <MemberPicker
        workspaceSlug="acme"
        projectId={PROJECT}
        onChange={onChange}
        onOpenChange={onOpenChange}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onChange, onOpenChange, ...view };
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

afterEach(() => {
  resetAppClient();
});

describe("MemberPicker", () => {
  it("fetches lazily on open and lists members You-first", async () => {
    const { queryClient, calls } = harness(memberRoutes());
    setupPicker(queryClient, { value: null });
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["You", "Ana", "Zoe", "Max"]);
    expect(screen.getByText("Suspended")).toBeInTheDocument();
    expect(document.querySelector(`img[src="${AVATAR}"]`)).not.toBeNull();
    expect(calls.some((url) => url.includes("/projects/") && url.endsWith("/members/"))).toBe(true);
    expect(calls.some((url) => url === "https://api.test/api/workspaces/acme/members/")).toBe(true);
    expect(screen.queryByRole("option", { name: "Parity Guest" })).not.toBeInTheDocument();
  });

  it("searches display names and names the empty state", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(memberRoutes());
    setupPicker(queryClient, { value: null });
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search members" }), "Parity");
    expect(optionNames()).toEqual(["You"]);
    await user.clear(screen.getByRole("combobox", { name: "Search members" }));
    await user.type(screen.getByRole("combobox", { name: "Search members" }), "Parity Guest");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("keeps multi-select open and toggles picks", async () => {
    const { queryClient } = harness(memberRoutes());
    const { onChange, rerender } = setupPicker(queryClient, { multiple: true, value: [] });
    fireEvent.click(screen.getByRole("button", { name: "Select members" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "You" }));
    expect(onChange).toHaveBeenCalledWith([ME]);
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[ME]} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("option", { name: "You" }));
    expect(onChange).toHaveBeenCalledWith([]);
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("closes single-select on pick and reports the id", async () => {
    const { queryClient } = harness(memberRoutes());
    const { onChange } = setupPicker(queryClient, { value: null });
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "Ana" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Ana" }));
    expect(onChange).toHaveBeenCalledWith(ANA);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("never activates suspended members", async () => {
    const { queryClient } = harness(memberRoutes());
    const { onChange } = setupPicker(queryClient, { value: null });
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    const suspended = await screen.findByRole("option", { name: /Zoe/ });
    expect(suspended).toBeDisabled();
    fireEvent.click(suspended);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("offers the directory alone without a project scope", async () => {
    const { queryClient, calls } = harness(memberRoutes());
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["You", "Ana", "Zoe", "Max"]);
    expect(calls.some((url) => url.includes("/projects/"))).toBe(false);
  });

  it("names project members missing from the directory", async () => {
    const routes = (url: string): Route => {
      if (url === "https://api.test/api/users/me/") return { status: 200, body: JSON.stringify(ME_PAYLOAD) };
      if (url.includes("/projects/")) return { status: 200, body: JSON.stringify([projectRow(MAX)]) };
      if (url.endsWith("/members/")) return { status: 200, body: JSON.stringify([]) };
      throw new Error(`unexpected url ${url}`);
    };
    const { queryClient } = harness(routes);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "Unknown member" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Unknown member" }));
    expect(onChange).toHaveBeenCalledWith(MAX);
  });

  it("shows a loading row while the reads resolve", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const routes = (url: string): Route => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: '{"csrf_token":"csrf"}' };
      return { status: 200, body: "[]", gate };
    };
    const { queryClient } = harness(routes, false);
    setupPicker(queryClient, { value: null });
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(screen.getByRole("status")).toHaveTextContent("Loading members");
    release();
    expect(await screen.findByText("No matching results")).toBeInTheDocument();
  });

  it("reports failed loads", async () => {
    const routes = (url: string): Route => {
      if (url === "https://api.test/api/users/me/") return { status: 200, body: JSON.stringify(ME_PAYLOAD) };
      return { status: 500, body: '{"detail":"boom"}' };
    };
    const { queryClient } = harness(routes);
    setupPicker(queryClient, { value: null });
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load members.");
  });

  it("blocks reads while disabled", () => {
    const { queryClient, calls } = harness(memberRoutes());
    setupPicker(queryClient, { value: null, disabled: true });
    expect(screen.getByRole("button", { name: "Select member" })).toBeDisabled();
    expect(calls).toEqual([]);
  });

  it("renders value triggers with display names", async () => {
    const { queryClient } = harness(memberRoutes());
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select member" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ME} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select member" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Parity User" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "You" }));
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ANA} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Ana" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ME} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Parity User" })).toBeInTheDocument();
  });

  it("renders multi triggers as avatar groups with counts", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(memberRoutes());
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[]} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select members" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[ME, ANA]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Parity User, Ana" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          multiple
          value={[ME, ANA, ZOE, MAX]}
          onChange={onChange}
        />
      </QueryClientProvider>
    );
    const trigger = screen.getByRole("button", { name: "4 members" });
    expect(trigger).toHaveTextContent("+1");
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[ME]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Parity User" })).toBeInTheDocument();
  });

  it("renders icon-only triggers", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(memberRoutes());
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ANA} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Ana" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          multiple
          value={[ME, ANA, ZOE, MAX]}
          onChange={onChange}
          iconOnly
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "4 members" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[]} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select members" })).toBeInTheDocument();
  });

  it("accepts custom triggers, placeholders and controlled open state", async () => {
    const { queryClient } = harness(memberRoutes());
    const onChange = vi.fn();
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={onChange}
          trigger={<button type="button">Pick</button>}
          placeholder="Choose"
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Pick" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={onChange}
          trigger={<button type="button">Pick</button>}
          placeholder="Choose"
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
  });

  it("waits for the project memberships before listing a project-scoped picker", () => {
    const { queryClient } = harness(memberRoutes());
    queryClient.setQueryData(pickerKeys.workspaceMembers("acme"), DIRECTORY);
    setupPicker(queryClient, { value: null, open: true });
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading members");
  });

  it("falls back to the prompt for unknown single values and avatar triggers", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(memberRoutes());
    const onChange = vi.fn();
    const missing = "99999999-9999-4999-8999-999999999999";
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={missing}
          onChange={onChange}
          placeholder="Choose"
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Choose" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Choose" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={missing}
          onChange={onChange}
          placeholder="Choose"
          iconOnly
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Choose" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={missing} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select member" })).toBeInTheDocument();
  });

  it("renders avatars on the option and trigger forms once the directory has loaded", () => {
    const { queryClient } = harness(memberRoutes());
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ANA} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    return screen.findByRole("option", { name: "Ana" }).then((option) => {
      expect(option.querySelector(`img[src="${AVATAR}"]`)).not.toBeNull();
      fireEvent.click(option);
      expect(screen.getByRole("button", { name: "Ana" }).querySelector(`img[src="${AVATAR}"]`)).not.toBeNull();
      rerender(
        <QueryClientProvider client={queryClient}>
          <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={ANA} onChange={() => undefined} iconOnly />
        </QueryClientProvider>
      );
      expect(screen.getByRole("button", { name: "Ana" }).querySelector(`img[src="${AVATAR}"]`)).not.toBeNull();
      rerender(
        <QueryClientProvider client={queryClient}>
          <MemberPicker
            workspaceSlug="acme"
            projectId={PROJECT}
            multiple
            value={[ANA, ZOE]}
            onChange={() => undefined}
          />
        </QueryClientProvider>
      );
      expect(screen.getByRole("button", { name: "Ana, Zoe" }).querySelectorAll("img")).toHaveLength(2);
      rerender(
        <QueryClientProvider client={queryClient}>
          <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={MAX} onChange={() => undefined} iconOnly />
        </QueryClientProvider>
      );
      const initials = screen.getByRole("button", { name: "Max" });
      expect(initials.querySelector("img")).toBeNull();
      expect(initials).toHaveTextContent("MA");
    });
  });

  it("opens without an open-change handler", async () => {
    const { queryClient } = harness(memberRoutes());
    render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select member" }));
    expect(await screen.findByRole("option", { name: "You" })).toBeInTheDocument();
  });

  it("sorts selected-first and renders without a signed-in member", async () => {
    const routes = (url: string): Route => {
      if (url === "https://api.test/api/users/me/") return { status: 500, body: '{"detail":"boom"}' };
      if (url.includes("/projects/")) return { status: 200, body: JSON.stringify([projectRow(ANA), projectRow(MAX)]) };
      if (url.endsWith("/members/")) {
        return { status: 200, body: JSON.stringify([directoryRow(ANA, "Ana"), directoryRow(MAX, "Max")]) };
      }
      throw new Error(`unexpected url ${url}`);
    };
    const { queryClient } = harness(routes, false);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <MemberPicker workspaceSlug="acme" projectId={PROJECT} multiple value={[MAX]} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "1 member" }));
    expect(await screen.findByRole("option", { name: "Ana" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Max", "Ana"]);
  });

  it("preloads to name a selection before the popup opens", async () => {
    const { queryClient, calls } = harness(memberRoutes());
    setupPicker(queryClient, { multiple: true, value: [ANA], preload: true });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Ana" })).toBeInTheDocument();
    expect(calls.some((url) => url.includes("/projects/") && url.endsWith("/members/"))).toBe(true);
    expect(calls.some((url) => url === "https://api.test/api/workspaces/acme/members/")).toBe(true);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("names the value on a disabled row without opening it", async () => {
    const { queryClient, calls } = harness(memberRoutes());
    setupPicker(queryClient, { value: ANA, preload: true, disabled: true });
    expect(await screen.findByRole("button", { name: "Ana" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Ana" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(calls.some((url) => url === "https://api.test/api/workspaces/acme/members/")).toBe(true);
  });
});
