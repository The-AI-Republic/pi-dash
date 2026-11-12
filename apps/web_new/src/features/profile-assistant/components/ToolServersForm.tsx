// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry (parity/specs/profile.md §10, PROF-023). Rows list
// the name, the endpoint, a disabled marker, a lock marker when a
// credential is stored (the credential itself is never returned), and the
// tool prefix an enabled row claims. Each row flips between enabled and
// disabled or leaves the list. Adding needs a name and an endpoint; the
// server keeps the list authoritative, so a refusal leaves every row in
// place and only reports.

import type { McpServer } from "@pidash/api-client";
import { Button, Input } from "@pidash/kit";
import * as React from "react";

import {
  ADD_SERVER_LABEL,
  AUTH_HEADER_LABEL,
  AUTH_HEADER_PLACEHOLDER,
  SERVER_DISABLED_MARKER,
  SERVER_LOCK_LABEL,
  SERVER_LOCK_MARKER,
  SERVER_NAME_LABEL,
  SERVER_NAME_PLACEHOLDER,
  SERVER_URL_LABEL,
  SERVER_URL_PLACEHOLDER,
  TOOLS_SECTION_DESCRIPTION,
  TOOLS_SECTION_HEADING,
  REMOVE_LABEL,
} from "../lib/copy.js";
import { canAddServer, serverPrefixLine } from "../lib/rules.js";
import { SectionFrame } from "./AssistantSections.js";

export interface ToolServerDraft {
  name: string;
  url: string;
  authHeader: string;
}

export function emptyToolServerDraft(): ToolServerDraft {
  return { name: "", url: "", authHeader: "" };
}

export interface ToolServersFormProps {
  servers: McpServer[] | undefined;
  draft: ToolServerDraft;
  onDraftChange: (draft: ToolServerDraft) => void;
  onAdd: () => void;
  onToggle: (server: McpServer) => void;
  onRemove: (server: McpServer) => void;
  adding: boolean;
  pendingId: string | null;
}

export function ToolServerRow({
  server,
  onToggle,
  onRemove,
  pending,
}: {
  server: McpServer;
  onToggle: (server: McpServer) => void;
  onRemove: (server: McpServer) => void;
  pending: boolean;
}): React.ReactElement {
  const prefixLine = server.is_enabled ? serverPrefixLine(server) : null;
  return (
    <li
      data-testid="tool-server-row"
      data-server-name={server.name}
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-4) py-(--space-3) sm:flex-row sm:items-start sm:justify-between"
    >
      <div className="flex min-w-0 flex-col gap-(--space-1)">
        <div className="flex items-center gap-(--space-2)">
          <span className="text-body truncate font-medium text-(--text)">{server.name}</span>
          {server.is_enabled ? null : <span className="text-meta text-(--text-muted)">{SERVER_DISABLED_MARKER}</span>}
          {server.has_auth_header ? (
            <span
              className="text-meta text-(--text-muted)"
              role="img"
              aria-label={SERVER_LOCK_LABEL}
              title={SERVER_LOCK_LABEL}
            >
              {SERVER_LOCK_MARKER}
            </span>
          ) : null}
        </div>
        <div data-testid="tool-server-url" className="text-meta truncate text-(--text-muted)">
          {server.url}
        </div>
        {prefixLine === null ? null : (
          <span className="text-meta text-(--text-muted)">
            {prefixLine.slice(0, prefixLine.lastIndexOf(" ") + 1)}
            <code className="font-mono">{prefixLine.slice(prefixLine.lastIndexOf(" ") + 1)}</code>
          </span>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-(--space-2)">
        <Button
          loading={pending}
          onClick={() => onToggle(server)}
          aria-label={`${server.is_enabled ? "Disable" : "Enable"} ${server.name}`}
        >
          {server.is_enabled ? "Disable" : "Enable"}
        </Button>
        <Button variant="danger" disabled={pending} onClick={() => onRemove(server)}>
          {REMOVE_LABEL}
        </Button>
      </div>
    </li>
  );
}

export function ToolServersForm({
  servers,
  draft,
  onDraftChange,
  onAdd,
  onToggle,
  onRemove,
  adding,
  pendingId,
}: ToolServersFormProps): React.ReactElement {
  const edit = (patch: Partial<ToolServerDraft>): void => onDraftChange({ ...draft, ...patch });
  return (
    <SectionFrame heading={TOOLS_SECTION_HEADING} description={TOOLS_SECTION_DESCRIPTION} testId="assistant-tools">
      {servers !== undefined && servers.length > 0 ? (
        <ul data-testid="tool-server-list" className="flex flex-col gap-(--space-2)">
          {servers.map((server) => (
            <ToolServerRow
              key={server.id}
              server={server}
              onToggle={onToggle}
              onRemove={onRemove}
              pending={pendingId === server.id}
            />
          ))}
        </ul>
      ) : null}

      <div className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)">
        <Input
          label={SERVER_NAME_LABEL}
          value={draft.name}
          placeholder={SERVER_NAME_PLACEHOLDER}
          onChange={(event) => edit({ name: event.target.value })}
        />
        <Input
          label={SERVER_URL_LABEL}
          value={draft.url}
          placeholder={SERVER_URL_PLACEHOLDER}
          onChange={(event) => edit({ url: event.target.value })}
        />
        <Input
          label={AUTH_HEADER_LABEL}
          type="password"
          autoComplete="off"
          value={draft.authHeader}
          placeholder={AUTH_HEADER_PLACEHOLDER}
          onChange={(event) => edit({ authHeader: event.target.value })}
        />
        <div>
          <Button variant="primary" loading={adding} disabled={adding || !canAddServer(draft)} onClick={onAdd}>
            {ADD_SERVER_LABEL}
          </Button>
        </div>
      </div>
    </SectionFrame>
  );
}
