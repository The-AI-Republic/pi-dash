// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));

vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import type { ViewAccessFieldProps } from "../../../core/edition/index.js";
import { ViewAccessField } from "./ViewAccessField.js";

afterEach(() => {
  harness.edition = { id: "oss", flags: {} };
});

describe("ViewAccessField", () => {
  it("renders nothing on OSS", () => {
    render(<ViewAccessField value={1} onChange={() => undefined} disabled={false} />);
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
  });

  it("renders the edition selector with the dialog's choice", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: {
        viewAccess: {
          Field: ({ value, onChange: pick, disabled }: ViewAccessFieldProps) => (
            <button type="button" disabled={disabled} onClick={() => pick(0)}>
              {value === 1 ? "Shared" : "Owner-only"}
            </button>
          ),
          storeAccess: vi.fn(async () => undefined),
        },
      },
    };
    render(<ViewAccessField value={1} onChange={onChange} disabled={false} />);
    await user.click(screen.getByRole("button", { name: "Shared" }));
    expect(onChange).toHaveBeenCalledWith(0);
  });

  it("disables the edition selector while submitting", () => {
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: {
        viewAccess: {
          Field: ({ disabled }: ViewAccessFieldProps) => (
            <button type="button" disabled={disabled}>
              Shared
            </button>
          ),
          storeAccess: vi.fn(async () => undefined),
        },
      },
    };
    render(<ViewAccessField value={1} onChange={() => undefined} disabled={true} />);
    expect(screen.getByRole("button", { name: "Shared" })).toBeDisabled();
  });
});
