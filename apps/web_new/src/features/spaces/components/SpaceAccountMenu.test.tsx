// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Me, type Transport } from "@pidash/api-client";
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import type { SpaceAuthServices } from "../auth/index.js";

import { SPACE_SIGN_OUT_ACTION, SpaceAccountMenu } from "./SpaceAccountMenu.js";

function viewerWith(overrides: Partial<Me> = {}): Me {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    email: "ada@example.test",
    username: "ada",
    first_name: "Ada",
    last_name: "Lovelace",
    display_name: "Ada",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2026-01-01T00:00:00Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "password",
    last_login_time: null,
    ...overrides,
  };
}

function servicesWith(token: Promise<string>): SpaceAuthServices {
  return {
    checkEmail: () => Promise.reject(new Error("unused")),
    generateCode: () => Promise.reject(new Error("unused")),
    readCsrfToken: () => token,
  };
}

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("SpaceAccountMenu", () => {
  it("shows the display name on the trigger", () => {
    render(
      <SpaceAccountMenu
        viewer={viewerWith()}
        anchor="anchor-1"
        search={{}}
        services={servicesWith(Promise.resolve("tok"))}
      />
    );
    expect(screen.getByTestId("space-account-trigger")).toHaveTextContent("Ada");
  });

  it("shows the uploaded avatar when one resolves", () => {
    const { container } = render(
      <SpaceAccountMenu
        viewer={viewerWith({ avatar_url: "https://cdn.test/ada.png" })}
        anchor="anchor-1"
        search={{}}
        services={servicesWith(Promise.resolve("tok"))}
      />
    );
    expect(container.querySelector('img[src="https://cdn.test/ada.png"]')).not.toBeNull();
  });

  it("falls back to first plus last name without a display name", () => {
    render(
      <SpaceAccountMenu
        viewer={viewerWith({ display_name: "" })}
        anchor="anchor-1"
        search={{}}
        services={servicesWith(Promise.resolve("tok"))}
      />
    );
    expect(screen.getByTestId("space-account-trigger")).toHaveTextContent("Ada Lovelace");
  });

  it("opens a sign-out-only form carrying the token and the position", async () => {
    render(
      <SpaceAccountMenu
        viewer={viewerWith()}
        anchor="anchor-1"
        search={{ board: "kanban", peekId: "issue-1" }}
        services={servicesWith(Promise.resolve("tok-1"))}
      />
    );
    fireEvent.click(screen.getByTestId("space-account-trigger"));
    const form = await screen.findByTestId("space-signout-form");
    expect(form).toHaveAttribute("method", "post");
    expect(form).toHaveAttribute("action", SPACE_SIGN_OUT_ACTION);
    expect(form.querySelector('input[name="csrfmiddlewaretoken"]')).toHaveAttribute("value", "tok-1");
    expect(form.querySelector('input[name="next_path"]')).toHaveAttribute(
      "value",
      "/issues/anchor-1/?board=kanban&peekId=issue-1"
    );
    expect(screen.getByRole("button", { name: "Sign out" })).toHaveAttribute("type", "submit");
  });

  it("keeps the menu empty until the token loads", async () => {
    render(
      <SpaceAccountMenu
        viewer={viewerWith()}
        anchor="anchor-1"
        search={{}}
        services={servicesWith(new Promise(() => undefined))}
      />
    );
    fireEvent.click(screen.getByTestId("space-account-trigger"));
    await screen.findByTestId("space-account-trigger");
    expect(screen.queryByTestId("space-signout-form")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sign out" })).not.toBeInTheDocument();
  });

  it("keeps the menu empty when the token read fails", async () => {
    render(
      <SpaceAccountMenu
        viewer={viewerWith()}
        anchor="anchor-1"
        search={{}}
        services={servicesWith(Promise.reject(new Error("down")))}
      />
    );
    fireEvent.click(screen.getByTestId("space-account-trigger"));
    await screen.findByTestId("space-account-trigger");
    expect(screen.queryByTestId("space-signout-form")).not.toBeInTheDocument();
  });

  it("reads the token through the default public client without services", async () => {
    const transport: Transport = async (url) => {
      if (url.includes("/auth/get-csrf-token/")) return reply(200, { csrf_token: "tok-default" });
      throw new Error(`unexpected request ${url}`);
    };
    runtime.client = createClient({ baseUrl: "https://api.test", transport });
    render(<SpaceAccountMenu viewer={viewerWith()} anchor="anchor-1" search={{}} />);
    fireEvent.click(screen.getByTestId("space-account-trigger"));
    const form = await screen.findByTestId("space-signout-form");
    expect(form.querySelector('input[name="csrfmiddlewaretoken"]')).toHaveAttribute("value", "tok-default");
  });
});
