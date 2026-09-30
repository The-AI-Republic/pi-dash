// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Avatar } from "./Avatar";
import { Badge } from "./Badge";
import { Kbd } from "./Kbd";
import { Skeleton } from "./Skeleton";
import { Spinner } from "./Spinner";

describe("Spinner", () => {
  it("announces itself as a live region", () => {
    render(<Spinner label="Saving" />);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Saving");
  });

  it("renders sizes without dropping the announcement", () => {
    const { rerender } = render(<Spinner size="small" />);
    for (const size of ["small", "medium", "large"] as const) {
      rerender(<Spinner size={size} />);
      expect(screen.getByRole("status")).toBeInTheDocument();
    }
  });
});

describe("Skeleton", () => {
  it("is hidden from assistive technology", () => {
    const { container } = render(<Skeleton width="120px" height="16px" />);
    const node = container.firstElementChild!;
    expect(node).toHaveAttribute("aria-hidden", "true");
    expect(node).toHaveStyle({ width: "120px", height: "16px" });
  });
});

describe("Kbd", () => {
  it("names the full shortcut and shows each key", () => {
    render(<Kbd keys={["mod", "K"]} />);
    expect(screen.getByLabelText("mod+K", { selector: "span" })).toBeInTheDocument();
    expect(screen.getByText("mod", { selector: "kbd" })).toBeInTheDocument();
    expect(screen.getByText("K", { selector: "kbd" })).toBeInTheDocument();
  });
});

describe("Badge", () => {
  it("renders the label with a status dot", () => {
    render(
      <Badge tone="success" dot>
        Active
      </Badge>
    );
    expect(screen.getByText("Active")).toBeInTheDocument();
  });

  it("renders counts without a dot", () => {
    const { container } = render(<Badge>3</Badge>);
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(container.querySelectorAll("span").length).toBe(1);
  });
});

describe("Avatar", () => {
  it("falls back to initials with the name", () => {
    render(<Avatar name="Ada Lovelace" />);
    const avatar = screen.getByRole("img", { name: "Ada Lovelace" });
    expect(avatar).toHaveTextContent("AL");
  });

  it("shows the image when one loads", () => {
    render(<Avatar name="Ada Lovelace" src="https://example.test/ada.png" />);
    expect(screen.getByRole("img", { name: "Ada Lovelace" }).querySelector("img")).not.toBeNull();
  });
});
