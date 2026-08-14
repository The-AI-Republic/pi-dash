/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import type { IAssistantMCPServer } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";

const service = new AssistantService();

type ApiError = { detail?: string; error?: string } | null;

const errorMessage = (e: unknown, fallback: string): string => {
  const err = e as ApiError;
  return err?.detail || err?.error || fallback;
};

export const AssistantMCPServersSettings = observer(function AssistantMCPServersSettings() {
  const { data: servers, mutate } = useSWR<IAssistantMCPServer[]>("assistant-mcp-servers", () =>
    service.listMCPServers()
  );

  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [authHeader, setAuthHeader] = useState("");
  const [adding, setAdding] = useState(false);

  const add = async () => {
    setAdding(true);
    try {
      await service.createMCPServer({
        name: name.trim(),
        url: url.trim(),
        auth_header: authHeader.trim() || undefined,
      });
      setName("");
      setUrl("");
      setAuthHeader("");
      await mutate();
      setToast({ type: TOAST_TYPE.SUCCESS, title: "Added", message: "Tool server added." });
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Could not add server",
        message: errorMessage(e, "Invalid tool server."),
      });
    } finally {
      setAdding(false);
    }
  };

  const toggle = async (server: IAssistantMCPServer) => {
    try {
      await service.updateMCPServer(server.id, { is_enabled: !server.is_enabled });
      await mutate();
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Update failed",
        message: errorMessage(e, "Could not update the tool server."),
      });
    }
  };

  const remove = async (server: IAssistantMCPServer) => {
    try {
      await service.deleteMCPServer(server.id);
      await mutate();
      setToast({ type: TOAST_TYPE.INFO, title: "Removed", message: `${server.name} removed.` });
    } catch (e: unknown) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Remove failed",
        message: errorMessage(e, "Could not remove the tool server."),
      });
    }
  };

  return (
    <div className="flex max-w-xl flex-col gap-5">
      <div>
        <h3 className="text-16 font-semibold text-primary">Tool servers (MCP)</h3>
        <p className="mt-1 text-13 text-secondary">
          Connect MCP servers to give the assistant extra tools. Their tools are available during every assistant run,
          alongside the built-in ones. A server that is unreachable is skipped for that run — it never blocks the
          assistant.
        </p>
      </div>

      {servers && servers.length > 0 && (
        <ul className="flex flex-col gap-2">
          {servers.map((server) => (
            <li
              key={server.id}
              className="flex items-center justify-between gap-3 rounded-md border border-subtle bg-surface-1 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="truncate text-13 font-medium text-primary">{server.name}</span>
                  {!server.is_enabled && <span className="text-11 text-secondary">(disabled)</span>}
                  {server.has_auth_header && <span className="text-11 text-secondary">🔒</span>}
                </div>
                <div className="truncate text-12 text-secondary">{server.url}</div>
                <div className="text-11 text-secondary">
                  Tools appear as <code>{server.tool_prefix}_*</code>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <Button variant="neutral-primary" onClick={() => toggle(server)}>
                  {server.is_enabled ? "Disable" : "Enable"}
                </Button>
                <Button variant="tertiary-danger" onClick={() => remove(server)}>
                  Remove
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-col gap-3 rounded-md border border-subtle p-3">
        <span className="text-13 font-medium text-primary">Add a tool server</span>

        <label className="flex flex-col gap-1 text-13">
          <span className="text-secondary">Name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="My tools"
            className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
          />
        </label>

        <label className="flex flex-col gap-1 text-13">
          <span className="text-secondary">Server URL</span>
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://example.com/mcp"
            className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
          />
        </label>

        <label className="flex flex-col gap-1 text-13">
          <span className="text-secondary">Authorization header (optional)</span>
          <input
            type="password"
            value={authHeader}
            onChange={(e) => setAuthHeader(e.target.value)}
            placeholder="Bearer …"
            className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
          />
        </label>

        <div>
          <Button onClick={add} loading={adding} disabled={!name.trim() || !url.trim()}>
            Add server
          </Button>
        </div>
      </div>
    </div>
  );
});
