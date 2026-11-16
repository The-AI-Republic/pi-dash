// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek address for a layout (parity/specs/issues.md §14, ISS-167): a
// controlled host decides what is mounted and receives every change; a
// standalone layout reads a shared link on mount and writes each change
// into the address with replace, so Back leaves the page.
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePeekAddress } from "./usePeekAddress.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "423e4567-e89b-12d3-a456-426614174003";
const CHILD = "523e4567-e89b-12d3-a456-426614174004";
const PATH = `/acme/projects/${PROJECT}/issues`;

function search(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}

beforeEach(() => {
  window.history.replaceState({}, "", PATH);
});

afterEach(() => {
  window.history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});

describe("usePeekAddress", () => {
  it("mounts what the controlled host names without touching the address", () => {
    const replace = vi.spyOn(window.history, "replaceState");
    const { result } = renderHook(() =>
      usePeekAddress({ projectId: PROJECT, peekKey: ISSUE, peekProjectId: null, onPeekChange: vi.fn() })
    );
    expect(result.current.peeked).toBe(ISSUE);
    expect(result.current.peekProjectId).toBeNull();
    act(() => result.current.open({ issueId: CHILD, projectId: OTHER, nestingLevel: 0 }));
    expect(replace).not.toHaveBeenCalled();
    expect(search().get("peekIssueId")).toBeNull();
  });

  it("reports open, nested changes and close to a controlled host", () => {
    const onPeekChange = vi.fn();
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT, peekKey: null, onPeekChange }));
    act(() => result.current.open({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 }));
    expect(onPeekChange).toHaveBeenLastCalledWith({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 });
    act(() => result.current.report({ issueId: CHILD, projectId: OTHER, nestingLevel: 1 }));
    expect(onPeekChange).toHaveBeenLastCalledWith({ issueId: CHILD, projectId: OTHER, nestingLevel: 1 });
    act(() => result.current.close());
    expect(onPeekChange).toHaveBeenLastCalledWith(null);
  });

  it("stays inert for a controlled address with no host writer", () => {
    const replace = vi.spyOn(window.history, "replaceState");
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT, peekKey: null }));
    act(() => result.current.open({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 }));
    act(() => result.current.report({ issueId: CHILD, projectId: PROJECT, nestingLevel: 1 }));
    act(() => result.current.close());
    expect(replace).not.toHaveBeenCalled();
    expect(result.current.peeked).toBeNull();
  });

  it("leaves a shared link to the host when the writer alone is supplied", () => {
    window.history.replaceState({}, "", `${PATH}?peekIssueId=${ISSUE}`);
    const onPeekChange = vi.fn();
    const replace = vi.spyOn(window.history, "replaceState");
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT, onPeekChange }));
    expect(result.current.peeked).toBeNull();
    act(() => result.current.open({ issueId: CHILD, projectId: OTHER, nestingLevel: 0 }));
    expect(onPeekChange).toHaveBeenCalledWith({ issueId: CHILD, projectId: OTHER, nestingLevel: 0 });
    expect(replace).not.toHaveBeenCalled();
  });

  it("opens a shared link on a standalone mount", () => {
    window.history.replaceState({}, "", `${PATH}?peekIssueId=${ISSUE}`);
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT }));
    expect(result.current.peeked).toBe(ISSUE);
    expect(result.current.peekProjectId).toBeNull();
  });

  it("writes a standalone open with replace and the child's project", () => {
    const push = vi.spyOn(window.history, "pushState");
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT }));
    act(() => result.current.open({ issueId: CHILD, projectId: OTHER, nestingLevel: 1 }));
    expect(push).not.toHaveBeenCalled();
    expect(search().get("peekIssueId")).toBe(CHILD);
    expect(search().get("peekProjectId")).toBe(OTHER);
    expect(search().get("peekNestingLevel")).toBe("1");
    expect(result.current.peekProjectId).toBe(OTHER);
  });

  it("never replaces a panel already up with a link read later", () => {
    const { result, rerender } = renderHook(
      ({ project }: { project: string }) => usePeekAddress({ projectId: project }),
      {
        initialProps: { project: PROJECT },
      }
    );
    act(() => result.current.open({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 }));
    window.history.replaceState({}, "", `${PATH}?peekIssueId=${CHILD}`);
    rerender({ project: OTHER });
    expect(result.current.peeked).toBe(ISSUE);
  });

  it("keeps the panel mounted while a nested child holds the address", () => {
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT }));
    act(() => result.current.open({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 }));
    act(() => result.current.report({ issueId: CHILD, projectId: PROJECT, nestingLevel: 1 }));
    expect(result.current.peeked).toBe(ISSUE);
    expect(search().get("peekNestingLevel")).toBe("1");
    act(() => result.current.report({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 }));
    expect(search().get("peekNestingLevel")).toBeNull();
  });

  it("strips every peek param on close and keeps the rest of the search", () => {
    window.history.replaceState({}, "", `${PATH}?layout=compact&peekProjectId=${OTHER}`);
    const { result } = renderHook(() => usePeekAddress({ projectId: PROJECT }));
    act(() => result.current.open({ issueId: ISSUE, projectId: OTHER, nestingLevel: 2 }));
    act(() => result.current.close());
    expect(result.current.peeked).toBeNull();
    expect(search().get("peekIssueId")).toBeNull();
    expect(search().get("peekProjectId")).toBeNull();
    expect(search().get("peekNestingLevel")).toBeNull();
    expect(search().get("layout")).toBe("compact");
  });
});
