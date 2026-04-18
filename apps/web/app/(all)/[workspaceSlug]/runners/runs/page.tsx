/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import useSWR from "swr";
import type { IAgentRun } from "@apple-pi-dash/services";
import { RunnerService } from "@apple-pi-dash/services";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

export default function RunnerRunsPage() {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data: runs, mutate } = useSWR<IAgentRun[]>(
    workspaceId ? ["runner-runs", workspaceId] : null,
    () => service.listRuns(workspaceId),
    { refreshInterval: 5_000 }
  );

  const [selected, setSelected] = useState<string | null>(null);
  const { data: detail } = useSWR<IAgentRun>(
    selected ? ["runner-run-detail", selected] : null,
    () => service.getRun(selected!, true),
    { refreshInterval: selected ? 3_000 : 0 }
  );

  async function cancel(id: string) {
    if (!confirm("Cancel this run?")) return;
    await service.cancelRun(id, "user");
    mutate();
  }

  return (
    <div className="grid grid-cols-[400px_1fr] gap-4">
      <div className="rounded-md border">
        <table className="text-sm w-full">
          <thead className="bg-neutral-50 text-left">
            <tr>
              <th className="px-3 py-2">Started</th>
              <th className="px-3 py-2">Status</th>
              <th className="px-3 py-2">Prompt</th>
            </tr>
          </thead>
          <tbody>
            {(runs ?? []).map((r) => (
              <tr
                key={r.id}
                onClick={() => setSelected(r.id)}
                className={`cursor-pointer border-t ${selected === r.id ? "bg-blue-50" : "hover:bg-neutral-50"}`}
              >
                <td className="px-3 py-2 whitespace-nowrap">{new Date(r.created_at).toLocaleString()}</td>
                <td className="px-3 py-2">
                  <RunStatus status={r.status} />
                </td>
                <td className="font-mono text-xs max-w-[180px] truncate px-3 py-2">{r.prompt}</td>
              </tr>
            ))}
            {(runs ?? []).length === 0 && (
              <tr>
                <td colSpan={3} className="text-neutral-500 px-3 py-8 text-center">
                  No runs yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="rounded-md border p-4">
        {!detail ? (
          <div className="text-sm text-neutral-500">Select a run on the left.</div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="font-mono text-xs">{detail.id}</div>
                <div className="mt-1">
                  <RunStatus status={detail.status} />
                </div>
              </div>
              {!["completed", "failed", "cancelled"].includes(detail.status) && (
                <button
                  onClick={() => cancel(detail.id)}
                  className="text-sm hover:bg-red-50 hover:text-red-600 rounded border px-3 py-1"
                >
                  Cancel run
                </button>
              )}
            </div>
            <div className="text-sm">
              <div className="text-neutral-500">Prompt</div>
              <pre className="bg-neutral-50 text-xs mt-1 rounded p-2 whitespace-pre-wrap">{detail.prompt}</pre>
            </div>
            {detail.error && (
              <div className="text-sm">
                <div className="text-red-600">Error</div>
                <pre className="bg-red-50 text-xs mt-1 rounded p-2 whitespace-pre-wrap">{detail.error}</pre>
              </div>
            )}
            {detail.done_payload && (
              <div className="text-sm">
                <div className="text-neutral-500">Done payload</div>
                <pre className="bg-neutral-50 text-xs mt-1 rounded p-2 whitespace-pre-wrap">
                  {JSON.stringify(detail.done_payload, null, 2)}
                </pre>
              </div>
            )}
            <div className="text-sm">
              <div className="text-neutral-500">Events ({detail.events?.length ?? 0})</div>
              <div className="mt-1 max-h-[420px] overflow-auto rounded border">
                <table className="text-xs w-full">
                  <thead className="bg-neutral-50 text-left">
                    <tr>
                      <th className="px-2 py-1">seq</th>
                      <th className="px-2 py-1">kind</th>
                      <th className="px-2 py-1">at</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(detail.events ?? []).map((e) => (
                      <tr key={e.id} className="border-t">
                        <td className="font-mono px-2 py-1">{e.seq}</td>
                        <td className="font-mono px-2 py-1">{e.kind}</td>
                        <td className="px-2 py-1">{new Date(e.created_at).toLocaleTimeString()}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function RunStatus({ status }: { status: string }) {
  const cls: Record<string, string> = {
    queued: "bg-neutral-100 text-neutral-700",
    assigned: "bg-blue-100 text-blue-700",
    running: "bg-blue-200 text-blue-800",
    awaiting_approval: "bg-yellow-100 text-yellow-800",
    awaiting_reauth: "bg-orange-100 text-orange-800",
    completed: "bg-green-100 text-green-800",
    failed: "bg-red-100 text-red-700",
    cancelled: "bg-neutral-200 text-neutral-700",
  };
  return <span className={`text-xs rounded px-2 py-0.5 ${cls[status] ?? "bg-neutral-100"}`}>{status}</span>;
}
