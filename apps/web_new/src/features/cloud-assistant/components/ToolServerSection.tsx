// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry section (parity/specs/cloud.md §7, CLOUD-036). The
// edition replaces the whole assistant tab, so the cloud page remounts the
// registry or cloud users lose the ability to register their own servers.
// Rows name the server, show its endpoint and claim its tool prefix; a
// locked row carries an auth header that never comes back from the server.

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { actionFailureDetail } from "../api/mutations.js";
import { useAddToolServer, useRemoveToolServer, useToolServers, useUpdateToolServer } from "../api/toolServers.js";
import {
  TOOL_SERVER_ADD_ACTION,
  TOOL_SERVER_ADD_FAILED_BODY,
  TOOL_SERVER_ADD_FAILED_TITLE,
  TOOL_SERVER_ADDED_BODY,
  TOOL_SERVER_ADDED_TITLE,
  TOOL_SERVER_AUTH_LABEL,
  TOOL_SERVER_AUTH_PLACEHOLDER,
  TOOL_SERVER_DISABLE_ACTION,
  TOOL_SERVER_DISABLED_MARKER,
  TOOL_SERVER_EMPTY,
  TOOL_SERVER_ENABLE_ACTION,
  TOOL_SERVERS_HEADING,
  TOOL_SERVERS_INTRO,
  TOOL_SERVER_LOCKED_MARKER,
  TOOL_SERVER_NAME_LABEL,
  TOOL_SERVER_NAME_PLACEHOLDER,
  TOOL_SERVER_REMOVE_FAILED_BODY,
  TOOL_SERVER_REMOVE_FAILED_TITLE,
  TOOL_SERVER_REMOVED_TITLE,
  TOOL_SERVER_UPDATE_FAILED_BODY,
  TOOL_SERVER_UPDATE_FAILED_TITLE,
  TOOL_SERVER_URL_LABEL,
  TOOL_SERVER_URL_PLACEHOLDER,
  REMOVE_ACTION,
} from "../lib/copy.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/index.js";

const FIELD =
  "h-(--control-height) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-body text-(--text)";

export function ToolServerSection(): React.ReactElement {
  const servers = useToolServers();
  const add = useAddToolServer();
  const update = useUpdateToolServer();
  const remove = useRemoveToolServer();
  const [name, setName] = React.useState("");
  const [url, setUrl] = React.useState("");
  const [authHeader, setAuthHeader] = React.useState("");

  const canAdd = !add.isPending && name.trim() !== "" && url.trim() !== "";

  const submit = (): void => {
    add.mutate(
      authHeader.trim() === ""
        ? { name: name.trim(), url: url.trim() }
        : { name: name.trim(), url: url.trim(), auth_header: authHeader.trim() },
      {
        onSuccess: () => {
          setName("");
          setUrl("");
          setAuthHeader("");
          notifySuccess(TOOL_SERVER_ADDED_TITLE, TOOL_SERVER_ADDED_BODY);
        },
        onError: (error) =>
          notifyError(TOOL_SERVER_ADD_FAILED_TITLE, actionFailureDetail(error, TOOL_SERVER_ADD_FAILED_BODY)),
      }
    );
  };

  return (
    <section aria-labelledby="cloud-tool-servers-heading" className="flex flex-col gap-(--space-4)">
      <div className="flex flex-col gap-(--space-1)">
        <h2 id="cloud-tool-servers-heading" className="text-h3 text-(--text)">
          {TOOL_SERVERS_HEADING}
        </h2>
        <p className="text-body text-(--text-muted)">{TOOL_SERVERS_INTRO}</p>
      </div>

      <div className="flex flex-col gap-(--space-2)">
        <label className="text-body flex flex-col gap-(--space-1)">
          <span className="text-(--text-muted)">{TOOL_SERVER_NAME_LABEL}</span>
          <input
            aria-label={TOOL_SERVER_NAME_LABEL}
            placeholder={TOOL_SERVER_NAME_PLACEHOLDER}
            value={name}
            onChange={(event) => setName(event.target.value)}
            className={FIELD}
          />
        </label>
        <label className="text-body flex flex-col gap-(--space-1)">
          <span className="text-(--text-muted)">{TOOL_SERVER_URL_LABEL}</span>
          <input
            aria-label={TOOL_SERVER_URL_LABEL}
            placeholder={TOOL_SERVER_URL_PLACEHOLDER}
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            className={FIELD}
          />
        </label>
        <label className="text-body flex flex-col gap-(--space-1)">
          <span className="text-(--text-muted)">{TOOL_SERVER_AUTH_LABEL}</span>
          <input
            aria-label={TOOL_SERVER_AUTH_LABEL}
            type="password"
            placeholder={TOOL_SERVER_AUTH_PLACEHOLDER}
            value={authHeader}
            onChange={(event) => setAuthHeader(event.target.value)}
            className={FIELD}
          />
        </label>
        <div>
          <Button variant="secondary" disabled={!canAdd} onClick={submit}>
            {TOOL_SERVER_ADD_ACTION}
          </Button>
        </div>
      </div>

      {servers.isPending ? <Spinner label="Loading tool servers" size="small" /> : null}
      {servers.data !== undefined && servers.data.length === 0 ? (
        <p className="text-body text-(--text-muted)">{TOOL_SERVER_EMPTY}</p>
      ) : null}
      {servers.data !== undefined && servers.data.length > 0 ? (
        <ul aria-label="Tool servers" className="flex flex-col gap-(--space-2)">
          {servers.data.map((server) => {
            const busy = update.isPending || remove.isPending;
            return (
              <li
                key={server.id}
                className="flex flex-wrap items-center justify-between gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-3)"
              >
                <div className="flex min-w-0 flex-col gap-(--space-1)">
                  <span className="text-body font-medium text-(--text)">
                    {server.name}
                    {server.is_enabled ? null : (
                      <span className="text-(--text-muted)"> {TOOL_SERVER_DISABLED_MARKER}</span>
                    )}
                    {server.has_auth_header ? (
                      <span className="text-caption ml-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) text-(--text-muted)">
                        {TOOL_SERVER_LOCKED_MARKER}
                      </span>
                    ) : null}
                  </span>
                  <div className="text-caption truncate text-(--text-muted)">{server.url}</div>
                  {server.effective_tool_prefix !== null ? (
                    <code className="text-caption text-(--accent)">{server.effective_tool_prefix}</code>
                  ) : null}
                </div>
                <div className="flex items-center gap-(--space-2)">
                  <Button
                    size="small"
                    variant="secondary"
                    disabled={busy}
                    onClick={() =>
                      update.mutate(
                        { serverId: server.id, patch: { is_enabled: !server.is_enabled } },
                        {
                          onError: (error) =>
                            notifyError(
                              TOOL_SERVER_UPDATE_FAILED_TITLE,
                              actionFailureDetail(error, TOOL_SERVER_UPDATE_FAILED_BODY)
                            ),
                        }
                      )
                    }
                  >
                    {server.is_enabled ? TOOL_SERVER_DISABLE_ACTION : TOOL_SERVER_ENABLE_ACTION}
                  </Button>
                  <Button
                    size="small"
                    variant="danger"
                    disabled={busy}
                    onClick={() =>
                      remove.mutate(server.id, {
                        onSuccess: () => notifyInfo(TOOL_SERVER_REMOVED_TITLE, `${server.name} removed.`),
                        onError: (error) =>
                          notifyError(
                            TOOL_SERVER_REMOVE_FAILED_TITLE,
                            actionFailureDetail(error, TOOL_SERVER_REMOVE_FAILED_BODY)
                          ),
                      })
                    }
                  >
                    {REMOVE_ACTION}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
