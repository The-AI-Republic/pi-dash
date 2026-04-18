/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import useSWR from "swr";
import { useState } from "react";
import type { IApprovalRequest } from "@apple-pi-dash/services";
import { RunnerService } from "@apple-pi-dash/services";

const service = new RunnerService();

export default function ApprovalsPage() {
  const { data: approvals, mutate } = useSWR<IApprovalRequest[]>("runner-approvals", () => service.listApprovals(), {
    refreshInterval: 2_000,
  });

  const [pending, setPending] = useState<string | null>(null);

  async function decide(id: string, decision: "accept" | "decline" | "accept_for_session") {
    setPending(id);
    try {
      await service.decideApproval(id, decision);
      mutate();
    } finally {
      setPending(null);
    }
  }

  const rows = approvals ?? [];
  if (rows.length === 0) {
    return <div className="text-sm text-neutral-500 rounded-md border p-8 text-center">No pending approvals.</div>;
  }
  return (
    <div className="flex flex-col gap-4">
      {rows.map((a) => (
        <div key={a.id} className="rounded-md border p-4">
          <div className="flex items-start justify-between">
            <div>
              <div className="text-xs text-neutral-500">
                Run {a.agent_run} · requested {new Date(a.requested_at).toLocaleTimeString()}
              </div>
              <div className="font-medium">{humanKind(a.kind)}</div>
              {a.reason && <div className="text-sm text-neutral-600 mt-1">{a.reason}</div>}
            </div>
            {a.expires_at && (
              <div className="text-xs text-neutral-500">expires {new Date(a.expires_at).toLocaleTimeString()}</div>
            )}
          </div>
          <pre className="bg-neutral-50 font-mono text-xs mt-3 rounded p-3 whitespace-pre-wrap">
            {JSON.stringify(a.payload, null, 2)}
          </pre>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => decide(a.id, "accept")}
              disabled={pending === a.id}
              className="bg-green-600 text-sm rounded px-3 py-1 font-medium text-white disabled:opacity-50"
            >
              Accept once
            </button>
            <button
              onClick={() => decide(a.id, "accept_for_session")}
              disabled={pending === a.id}
              className="bg-green-500 text-sm rounded px-3 py-1 font-medium text-white disabled:opacity-50"
            >
              Accept for session
            </button>
            <button
              onClick={() => decide(a.id, "decline")}
              disabled={pending === a.id}
              className="bg-red-600 text-sm rounded px-3 py-1 font-medium text-white disabled:opacity-50"
            >
              Decline
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

function humanKind(kind: IApprovalRequest["kind"]): string {
  switch (kind) {
    case "command_execution":
      return "Codex wants to run a shell command";
    case "file_change":
      return "Codex wants to modify a file";
    case "network_access":
      return "Codex wants to make a network call";
    default:
      return "Codex is requesting approval";
  }
}
