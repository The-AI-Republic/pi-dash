// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import {
  countLabel,
  labelNamesTooltip,
  ReadOnlyDate,
  ReadOnlyLabels,
  ReadOnlyMembers,
  ReadOnlyProperty,
  readOnlyValueText,
  READONLY_NONE,
} from "./ReadOnlyProperty.js";

describe("readOnlyValueText", () => {
  it("renders None for null, undefined and blank values", () => {
    expect(readOnlyValueText(null)).toBe(READONLY_NONE);
    expect(readOnlyValueText(undefined)).toBe(READONLY_NONE);
    expect(readOnlyValueText("   ")).toBe(READONLY_NONE);
    expect(readOnlyValueText("Done")).toBe("Done");
  });
});

describe("ReadOnlyProperty", () => {
  it("renders the value with its icon and no interactive control", () => {
    const { container } = render(<ReadOnlyProperty kind="state" value="In progress" />);
    expect(screen.getByText("In progress")).toBeInTheDocument();
    expect(container.querySelector('[data-readonly-property="state"]')).not.toBeNull();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(container.querySelectorAll("button, input, [role=button], [tabindex]")).toHaveLength(0);
  });

  it("falls back to None without a tooltip", () => {
    render(<ReadOnlyProperty kind="priority" value={null} />);
    expect(screen.getByText(READONLY_NONE)).toBeInTheDocument();
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("keeps a tooltip on the value when one is supplied", async () => {
    render(<ReadOnlyProperty kind="labels" value="2 labels" tooltip="Bug, UX" />);
    const value = screen.getByText("2 labels");
    fireEvent.focus(value);
    fireEvent.mouseEnter(value);
    expect(await screen.findByText("Bug, UX")).toBeInTheDocument();
  });

  it("accepts a custom class for the value row", () => {
    const { container } = render(<ReadOnlyProperty kind="module" value="Auth" className="custom-row" />);
    expect(container.querySelector(".custom-row")).not.toBeNull();
  });
});

describe("countLabel and labelNamesTooltip", () => {
  it("pluralizes by count", () => {
    expect(countLabel(0, "label")).toBe("0 labels");
    expect(countLabel(1, "label")).toBe("1 label");
    expect(countLabel(3, "module")).toBe("3 modules");
  });

  it("offers no tooltip without names", () => {
    expect(labelNamesTooltip([])).toBeUndefined();
    expect(labelNamesTooltip(["Bug", "UX"])).toBe("Bug, UX");
  });
});

describe("ReadOnlyMembers", () => {
  it("lists every member name and caps the avatars", () => {
    const { container } = render(
      <ReadOnlyMembers
        members={[
          { id: "1", name: "Ada" },
          { id: "2", name: "Grace" },
          { id: "3", name: "Linus" },
          { id: "4", name: "Barbara" },
        ]}
      />
    );
    expect(screen.getByText("Ada, Grace, Linus, Barbara")).toBeInTheDocument();
    expect(screen.getByText("+1")).toBeInTheDocument();
    expect(container.querySelectorAll('[role="img"]')).toHaveLength(3);
  });

  it("shows None for an empty assignment", () => {
    render(<ReadOnlyMembers members={[]} />);
    expect(screen.getByText(READONLY_NONE)).toBeInTheDocument();
  });
});

describe("ReadOnlyLabels", () => {
  it("shows a count with the names on hover", () => {
    render(<ReadOnlyLabels names={["Bug", "UX"]} />);
    expect(screen.getByText("2 labels")).toBeInTheDocument();
  });

  it("shows None without labels", () => {
    render(<ReadOnlyLabels names={[]} />);
    expect(screen.getByText(READONLY_NONE)).toBeInTheDocument();
  });
});

describe("ReadOnlyDate", () => {
  it("shows the ISO day or None", () => {
    const { unmount } = render(<ReadOnlyDate iso="2026-03-04" />);
    expect(screen.getByText("2026-03-04")).toBeInTheDocument();
    unmount();
    render(<ReadOnlyDate iso={null} />);
    expect(screen.getByText(READONLY_NONE)).toBeInTheDocument();
  });
});
