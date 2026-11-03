/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { InstanceRunnerService } from "@pi-dash/services";
import type { IInstanceRunnersPage } from "@pi-dash/types";
import { Button, Loader } from "@pi-dash/ui";
// components
import { PageWrapper } from "@/components/common/page-wrapper";

const service = new InstanceRunnerService();

const PER_PAGE = 50;

const PROVISIONING_OPTIONS = [
  { value: "", label: "all" },
  { value: "manual", label: "manual" },
  { value: "desktop_bundled", label: "desktop_bundled" },
];

const InstanceRunnersPage = observer(function InstanceRunnersPage() {
  const [provisioningFilter, setProvisioningFilter] = useState("");
  const [page, setPage] = useState(1);

  const { data, isLoading } = useSWR<IInstanceRunnersPage>(["INSTANCE_RUNNERS", provisioningFilter, page], () =>
    service.list({ page, ...(provisioningFilter ? { provisioning: provisioningFilter } : {}) })
  );

  const hasNextPage = !!data && data.page * PER_PAGE < data.total;

  return (
    <PageWrapper
      header={{
        title: "Runners",
        description: "All runners on this instance, with the agent build each user is on.",
      }}
      size="lg"
    >
      <div className="mx-4 space-y-4">
        <div className="flex items-center gap-2">
          <span className="text-12 text-secondary">Provisioning</span>
          <select
            className="rounded-md border border-subtle bg-surface-1 px-2 py-1 text-12"
            value={provisioningFilter}
            onChange={(e) => {
              setProvisioningFilter(e.target.value);
              setPage(1);
            }}
          >
            {PROVISIONING_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          {data && <span className="text-12 text-secondary">{data.total} total</span>}
        </div>

        {isLoading && !data ? (
          <Loader className="space-y-4">
            <Loader.Item height="48px" />
            <Loader.Item height="120px" />
          </Loader>
        ) : (
          <div className="overflow-hidden rounded-md border border-subtle">
            <table className="w-full text-body-sm-regular">
              <thead className="bg-surface-2 text-secondary">
                <tr>
                  <th className="px-3 py-2 text-left font-medium">Name</th>
                  <th className="px-3 py-2 text-left font-medium">Workspace</th>
                  <th className="px-3 py-2 text-left font-medium">Owner</th>
                  <th className="px-3 py-2 text-left font-medium">Provisioning</th>
                  <th className="px-3 py-2 text-left font-medium">Status</th>
                  <th className="px-3 py-2 text-left font-medium">Runner version</th>
                  <th className="px-3 py-2 text-left font-medium">Codex version</th>
                  <th className="px-3 py-2 text-left font-medium">Last heartbeat</th>
                </tr>
              </thead>
              <tbody>
                {(data?.results ?? []).map((r) => (
                  <tr key={r.id} className="border-t border-subtle">
                    <td className="px-3 py-2">{r.name}</td>
                    <td className="px-3 py-2">{r.workspace_slug ?? "—"}</td>
                    <td className="px-3 py-2 text-12 text-secondary">{r.owner_email ?? "—"}</td>
                    <td className="px-3 py-2 text-12">{r.provisioning}</td>
                    <td className="px-3 py-2 text-12">{r.status}</td>
                    <td className="px-3 py-2 text-12 text-secondary">{r.runner_version || "—"}</td>
                    <td className="px-3 py-2 text-12 text-secondary">{r.codex_version || "—"}</td>
                    <td className="px-3 py-2 text-12 text-secondary">
                      {r.last_heartbeat_at ? new Date(r.last_heartbeat_at).toLocaleString() : "—"}
                    </td>
                  </tr>
                ))}
                {data && data.results.length === 0 && (
                  <tr>
                    <td colSpan={8} className="px-3 py-4 text-center text-12 text-secondary">
                      No runners yet.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        )}

        {data && (data.page > 1 || hasNextPage) && (
          <div className="flex items-center gap-2">
            <Button
              variant="neutral-primary"
              disabled={data.page <= 1}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
            >
              Previous
            </Button>
            <span className="text-12 text-secondary">Page {data.page}</span>
            <Button variant="neutral-primary" disabled={!hasNextPage} onClick={() => setPage((p) => p + 1)}>
              Next
            </Button>
          </div>
        )}
      </div>
    </PageWrapper>
  );
});

export const meta = () => [{ title: "Runners - God Mode" }];

export default InstanceRunnersPage;
