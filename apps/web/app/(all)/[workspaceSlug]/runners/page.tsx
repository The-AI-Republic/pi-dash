/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import useSWR from "swr";
import type { IRunner } from "@apple-pi-dash/services";
import { RunnerService } from "@apple-pi-dash/services";
import { useWorkspace } from "@/hooks/store/use-workspace";

const service = new RunnerService();

const MAX_RUNNERS_PER_USER = 5;

export default function RunnersListPage() {
  const { currentWorkspace } = useWorkspace();
  const workspaceId = currentWorkspace?.id;

  const { data: runners, mutate } = useSWR<IRunner[]>(
    workspaceId ? ["runners", workspaceId] : null,
    () => service.list(workspaceId),
    { refreshInterval: 5_000 }
  );

  const [mintedToken, setMintedToken] = useState<string | null>(null);
  const [mintError, setMintError] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [minting, setMinting] = useState(false);

  const activeCount = (runners ?? []).filter((r) => r.status !== "revoked").length;
  const atCap = activeCount >= MAX_RUNNERS_PER_USER;

  async function mint() {
    if (!workspaceId) return;
    setMinting(true);
    setMintError(null);
    setMintedToken(null);
    try {
      const result = await service.mintToken(workspaceId, label || undefined);
      setMintedToken(result.token);
      setLabel("");
      mutate();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setMintError(err?.error ?? "failed to mint token");
    } finally {
      setMinting(false);
    }
  }

  async function revoke(runnerId: string) {
    if (!confirm("Revoke this runner? The daemon will be forced offline.")) return;
    await service.revoke(runnerId);
    mutate();
  }

  return (
    <div className="flex flex-col gap-6">
      <section className="rounded-md border p-4">
        <div className="flex items-center justify-between">
          <div>
            <div className="font-medium">Add a runner</div>
            <div className="text-sm text-neutral-500">
              You have {activeCount} of {MAX_RUNNERS_PER_USER} runners registered.
            </div>
          </div>
        </div>
        <div className="mt-3 flex items-center gap-2">
          <input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="optional label (e.g. my-laptop)"
            className="text-sm flex-1 rounded border px-3 py-1"
          />
          <button
            onClick={mint}
            disabled={!workspaceId || minting || atCap}
            className="bg-blue-600 text-sm rounded px-4 py-1 font-medium text-white disabled:opacity-50"
          >
            {minting ? "Minting…" : atCap ? "Cap reached" : "Mint registration code"}
          </button>
        </div>
        {mintError && <div className="text-sm text-red-600 mt-2">{mintError}</div>}
        {mintedToken && (
          <div className="border-yellow-300 bg-yellow-50 text-sm mt-3 rounded border p-3">
            <div className="font-medium">Copy this once — it will not be shown again.</div>
            <pre className="font-mono text-xs mt-2 break-all select-all">{mintedToken}</pre>
            <div className="text-neutral-600 mt-2">
              Run on your machine:
              <pre className="font-mono text-xs mt-1 whitespace-pre-wrap select-all">
                apple-pi-dash-runner configure --url {window.location.origin} --token {mintedToken}
              </pre>
            </div>
          </div>
        )}
      </section>

      <section>
        <div className="text-sm mb-2 font-medium">Connected runners</div>
        <div className="overflow-x-auto rounded-md border">
          <table className="text-sm w-full">
            <thead className="bg-neutral-50 text-left">
              <tr>
                <th className="px-3 py-2">Name</th>
                <th className="px-3 py-2">Status</th>
                <th className="px-3 py-2">OS / Arch</th>
                <th className="px-3 py-2">Version</th>
                <th className="px-3 py-2">Last heartbeat</th>
                <th className="px-3 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {(runners ?? []).map((r) => (
                <tr key={r.id} className="border-t">
                  <td className="font-mono text-xs px-3 py-2">{r.name}</td>
                  <td className="px-3 py-2">
                    <StatusBadge status={r.status} />
                  </td>
                  <td className="px-3 py-2">
                    {r.os} / {r.arch}
                  </td>
                  <td className="px-3 py-2">{r.runner_version || "—"}</td>
                  <td className="px-3 py-2">
                    {r.last_heartbeat_at ? new Date(r.last_heartbeat_at).toLocaleString() : "—"}
                  </td>
                  <td className="px-3 py-2 text-right">
                    {r.status !== "revoked" && (
                      <button
                        onClick={() => revoke(r.id)}
                        className="text-xs hover:bg-red-50 hover:text-red-600 rounded border px-2 py-1"
                      >
                        Revoke
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {(runners ?? []).length === 0 && (
                <tr>
                  <td colSpan={6} className="text-neutral-500 px-3 py-8 text-center">
                    No runners yet. Mint a registration code to connect your first one.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

function StatusBadge({ status }: { status: IRunner["status"] }) {
  const cls: Record<IRunner["status"], string> = {
    online: "bg-green-100 text-green-800",
    busy: "bg-blue-100 text-blue-800",
    offline: "bg-neutral-100 text-neutral-600",
    revoked: "bg-red-100 text-red-700",
  };
  return <span className={`text-xs rounded px-2 py-0.5 ${cls[status]}`}>{status}</span>;
}
