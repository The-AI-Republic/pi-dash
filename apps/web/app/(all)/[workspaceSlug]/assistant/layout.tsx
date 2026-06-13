/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { MessageSquare, SquarePen, Sparkles } from "lucide-react";
import { NavLink, Outlet, useParams } from "react-router";
import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IAssistantThread } from "@pi-dash/types";
import { cn } from "@pi-dash/utils";

const service = new AssistantService();

const navItemClass = (isActive: boolean, extra?: string) =>
  cn(
    "flex items-center gap-2 rounded px-2 text-13",
    extra,
    isActive ? "bg-layer-1 font-medium text-primary" : "text-secondary hover:bg-layer-1"
  );

const AssistantLayout = observer(function AssistantLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { data: threads } = useSWR<IAssistantThread[]>(
    workspaceSlug ? ["assistant-threads", workspaceSlug] : null,
    () => service.listThreads(workspaceSlug!)
  );

  // listThreads already excludes archived threads server-side.
  const base = `/${workspaceSlug}/assistant`;

  return (
    <div className="flex h-full w-full overflow-hidden">
      <aside className="flex w-[280px] shrink-0 flex-col border-r border-subtle bg-surface-1">
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-subtle px-4 text-14 font-semibold text-primary">
          <Sparkles className="size-4 text-accent-primary" />
          Pi Dash AI
        </div>
        <div className="shrink-0 p-2">
          <NavLink to={base} end className={({ isActive }) => navItemClass(isActive, "h-9")}>
            <SquarePen className="size-4" />
            <span>New chat</span>
          </NavLink>
        </div>
        <nav className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-2 pt-0">
          <div className="px-2 py-1 text-11 font-medium text-tertiary uppercase">Chats</div>
          {(threads ?? []).map((thread) => (
            <NavLink
              key={thread.id}
              to={`${base}/${thread.id}`}
              className={({ isActive }) => navItemClass(isActive, "min-h-9 py-2")}
            >
              <MessageSquare className="size-4 shrink-0" />
              <span className="min-w-0 flex-1 truncate">{thread.title || "Untitled conversation"}</span>
            </NavLink>
          ))}
          {threads && threads.length === 0 && (
            <div className="px-2 py-4 text-12 text-tertiary">No conversations yet.</div>
          )}
        </nav>
      </aside>
      <main className="min-w-0 flex-1 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
});

export default AssistantLayout;
