// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal tool-server registry (parity/specs/profile.md §10, PROF-023).
// The section owns its own read, writes and draft so any page that needs
// the registry mounts this one component: the open-source assistant tab
// and the edition tab that replaces it share the same behaviour and cache.

import * as React from "react";

import { useAddToolServer, useRemoveToolServer, useToggleToolServer } from "../api/mutations.js";
import { useAssistantServers } from "../api/queries.js";
import { emptyToolServerDraft, ToolServersForm, type ToolServerDraft } from "./ToolServersForm.js";

export function ToolServersSection(): React.ReactElement {
  const servers = useAssistantServers();
  const addServer = useAddToolServer();
  const toggleServer = useToggleToolServer();
  const removeServer = useRemoveToolServer();
  const [draft, setDraft] = React.useState<ToolServerDraft>(emptyToolServerDraft);
  const [pendingId, setPendingId] = React.useState<string | null>(null);

  const add = (): void => {
    const auth = draft.authHeader.trim();
    addServer.mutate(
      {
        name: draft.name.trim(),
        url: draft.url.trim(),
        ...(auth === "" ? {} : { auth_header: auth }),
      },
      { onSuccess: () => setDraft(emptyToolServerDraft()) }
    );
  };

  const toggle = (serverId: string, isEnabled: boolean): void => {
    setPendingId(serverId);
    toggleServer.mutate({ serverId, isEnabled }, { onSettled: () => setPendingId(null) });
  };

  const remove = (serverId: string, name: string): void => {
    setPendingId(serverId);
    removeServer.mutate({ serverId, name }, { onSettled: () => setPendingId(null) });
  };

  return (
    <ToolServersForm
      servers={servers.data}
      draft={draft}
      onDraftChange={setDraft}
      onAdd={add}
      onToggle={(server) => toggle(server.id, !server.is_enabled)}
      onRemove={(server) => remove(server.id, server.name)}
      adding={addServer.isPending}
      pendingId={pendingId}
    />
  );
}
