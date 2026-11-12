// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the gate and guard: branch selection and session
// bouncing belong to the admin area. What this suite owns is the search
// schema plus prop passing and the guard wrap.
vi.mock("../../features/admin/components/AdminGuards.js", () => ({
  AdminHomeGuard: ({ children }: { children: React.ReactNode }) => <div data-testid="admin-home-guard">{children}</div>,
}));
vi.mock("../../features/admin/components/AdminEntryGate.js", () => ({
  AdminEntryGate: ({ search }: { search: Record<string, string | undefined> }) => (
    <div
      data-testid="admin-gate"
      data-first={search.first_name ?? ""}
      data-last={search.last_name ?? ""}
      data-company={search.company ?? ""}
      data-company-name={search.company_name ?? ""}
      data-email={search.email ?? ""}
      data-telemetry={search.is_telemetry_enabled ?? ""}
      data-code={search.error_code ?? ""}
      data-message={search.error_message ?? ""}
    />
  ),
}));

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));

vi.mock("../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { parseAdminEntrySearch, Route } from "./index.js";

describe("parseAdminEntrySearch", () => {
  it("keeps every known param", () => {
    expect(
      parseAdminEntrySearch({
        first_name: "Parity",
        last_name: "Admin",
        company: "Prefill Co",
        company_name: "Server Co",
        email: "a@example.com",
        is_telemetry_enabled: "False",
        error_code: "5180",
        error_message: "taken",
      })
    ).toEqual({
      first_name: "Parity",
      last_name: "Admin",
      company: "Prefill Co",
      company_name: "Server Co",
      email: "a@example.com",
      is_telemetry_enabled: "False",
      error_code: "5180",
      error_message: "taken",
    });
  });

  it("drops empty and unknown params instead of throwing", () => {
    expect(parseAdminEntrySearch({})).toEqual({});
    expect(
      parseAdminEntrySearch({
        first_name: "",
        last_name: null,
        company_name: "",
        email: "",
        error_message: "",
        next: "/elsewhere",
      })
    ).toEqual({});
  });

  it("reads router-parsed numbers and booleans by their URL spelling", () => {
    // The router's JSON parser turns `error_code=5180` into a number and
    // `is_telemetry_enabled=False` stays a string; both must survive so a
    // numeric rejection is never swallowed (NEWFRONT-457).
    expect(parseAdminEntrySearch({ error_code: 5180, company: 42, is_telemetry_enabled: false })).toEqual({
      error_code: "5180",
      company: "42",
      is_telemetry_enabled: "false",
    });
    expect(parseAdminEntrySearch({ error_code: Number.NaN })).toEqual({});
    // A literal `true` on the wire parses to a boolean; it keeps its spelling
    // rather than being dropped like an empty value.
    expect(parseAdminEntrySearch({ is_telemetry_enabled: true, error_code: false })).toEqual({
      is_telemetry_enabled: "true",
      error_code: "false",
    });
  });

  it("keeps a partial search", () => {
    expect(parseAdminEntrySearch({ email: "a@example.com", error_code: "5175" })).toEqual({
      email: "a@example.com",
      error_code: "5175",
    });
  });
});

describe("god-mode entry route component", () => {
  afterEach(() => {
    harness.edition = { id: "oss", flags: {} };
    vi.restoreAllMocks();
  });

  function renderEntry(search: Record<string, string>): void {
    vi.spyOn(Route, "useSearch").mockReturnValue(search);
    const Component = Route.options.component;
    if (!Component) throw new Error("god-mode entry route has no component");
    render(<Component />);
  }

  it("wraps the gate in the home guard", () => {
    renderEntry({});
    expect(screen.getByTestId("admin-home-guard")).toContainElement(screen.getByTestId("admin-gate"));
  });

  it("passes the redirect search through to the gate", () => {
    renderEntry({ email: "a@example.com", error_code: "5175", error_message: "failed" });
    const gate = screen.getByTestId("admin-gate");
    expect(gate).toHaveAttribute("data-email", "a@example.com");
    expect(gate).toHaveAttribute("data-code", "5175");
    expect(gate).toHaveAttribute("data-message", "failed");
    expect(gate).toHaveAttribute("data-first", "");
  });

  it("renders the slotted cloud entry instead of the gate when an edition fills it", () => {
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: { godModeCloudEntry: () => <div data-testid="cloud-entry" /> },
    };
    renderEntry({});
    expect(screen.getByTestId("admin-home-guard")).toContainElement(screen.getByTestId("cloud-entry"));
    expect(screen.queryByTestId("admin-gate")).not.toBeInTheDocument();
  });
});
