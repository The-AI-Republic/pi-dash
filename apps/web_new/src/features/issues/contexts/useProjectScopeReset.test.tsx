// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archives tabs reset project-scoped state on a project switch but
// never on the first mount (parity/specs/archives.md §2.2/§2.5).
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { useProjectScopeReset } from "./useProjectScopeReset.js";

function Probe({ scope, onScopeChange }: { scope: string; onScopeChange: () => void }): React.ReactElement {
  useProjectScopeReset(scope, onScopeChange);
  return <div data-testid="probe" />;
}

describe("useProjectScopeReset", () => {
  it("does not fire on the first mount", () => {
    const onScopeChange = vi.fn();
    render(<Probe scope="acme/p1" onScopeChange={onScopeChange} />);
    expect(onScopeChange).not.toHaveBeenCalled();
  });

  it("fires once per project switch", () => {
    const onScopeChange = vi.fn();
    const { rerender } = render(<Probe scope="acme/p1" onScopeChange={onScopeChange} />);
    rerender(<Probe scope="acme/p2" onScopeChange={onScopeChange} />);
    expect(onScopeChange).toHaveBeenCalledTimes(1);
    rerender(<Probe scope="acme/p2" onScopeChange={onScopeChange} />);
    expect(onScopeChange).toHaveBeenCalledTimes(1);
    rerender(<Probe scope="acme/p3" onScopeChange={onScopeChange} />);
    expect(onScopeChange).toHaveBeenCalledTimes(2);
  });

  it("uses the latest callback without re-running for a new identity", () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = render(<Probe scope="acme/p1" onScopeChange={first} />);
    rerender(<Probe scope="acme/p1" onScopeChange={second} />);
    expect(first).not.toHaveBeenCalled();
    expect(second).not.toHaveBeenCalled();
    rerender(<Probe scope="acme/p2" onScopeChange={second} />);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
