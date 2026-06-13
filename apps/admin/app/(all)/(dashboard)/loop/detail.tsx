/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { useNavigate, useParams } from "react-router";
import useSWR from "swr";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { InstanceLoopService } from "@pi-dash/services";
import type { ILoopJob, ILoopTargetsPage } from "@pi-dash/types";
import { Button, Loader } from "@pi-dash/ui";
// components
import { PageWrapper } from "@/components/common/page-wrapper";
// local
import { LoopJobFormModal } from "./form-modal";

const service = new InstanceLoopService();

const SKIP_REASONS = [
  "",
  "user_disabled",
  "master_paused",
  "min_role",
  "llm_config_missing",
  "membership_gone",
  "turn_active",
  "dispatch_error",
];

const InstanceLoopDetailPage = observer(function InstanceLoopDetailPage() {
  const { jobId } = useParams<{ jobId: string }>();
  const navigate = useNavigate();
  const [editing, setEditing] = useState(false);
  const [skipFilter, setSkipFilter] = useState("");

  const { data: job, mutate } = useSWR<ILoopJob>(jobId ? ["INSTANCE_LOOP_JOB", jobId] : null, () =>
    service.retrieve(jobId!)
  );
  const { data: targets } = useSWR<ILoopTargetsPage>(jobId ? ["INSTANCE_LOOP_TARGETS", jobId, skipFilter] : null, () =>
    service.listTargets(jobId!, skipFilter ? { skip_reason: skipFilter } : {})
  );

  const remove = async () => {
    if (!job) return;
    try {
      await service.destroy(job.id);
      setToast({ type: TOAST_TYPE.SUCCESS, title: "Deleted", message: "Loop job deleted." });
      navigate("/loop/");
    } catch {
      setToast({ type: TOAST_TYPE.ERROR, title: "Delete failed", message: "Could not delete the job." });
    }
  };

  if (!job) {
    return (
      <PageWrapper header={{ title: "Loop job", description: "" }}>
        <Loader className="space-y-4">
          <Loader.Item height="48px" />
          <Loader.Item height="120px" />
        </Loader>
      </PageWrapper>
    );
  }

  return (
    <PageWrapper
      header={{
        title: job.name,
        description: job.public_description || "Loop job detail",
        actions: (
          <div className="flex gap-2">
            <Button variant="neutral-primary" onClick={() => setEditing(true)}>
              Edit
            </Button>
            <Button variant="link-danger" onClick={remove}>
              Delete
            </Button>
          </div>
        ),
      }}
    >
      <div className="mx-4 space-y-6">
        {job.stats && (
          <div className="grid grid-cols-4 gap-3">
            <Stat label="Targets" value={job.stats.target_count} />
            <Stat label="Completed (24h)" value={job.stats.completed} />
            <Stat label="Failed (24h)" value={job.stats.failed} />
            <Stat label="Skipped (24h)" value={job.stats.skipped} />
          </div>
        )}

        <div className="flex items-center gap-2">
          <span className="text-12 text-secondary">Skip reason</span>
          <select
            className="rounded-md border border-subtle bg-surface-1 px-2 py-1 text-12"
            value={skipFilter}
            onChange={(e) => setSkipFilter(e.target.value)}
          >
            {SKIP_REASONS.map((r) => (
              <option key={r} value={r}>
                {r || "all"}
              </option>
            ))}
          </select>
        </div>

        <div className="overflow-hidden rounded-md border border-subtle">
          <table className="w-full text-body-sm-regular">
            <thead className="bg-surface-2 text-secondary">
              <tr>
                <th className="px-3 py-2 text-left font-medium">Workspace</th>
                <th className="px-3 py-2 text-left font-medium">User</th>
                <th className="px-3 py-2 text-left font-medium">Next run</th>
                <th className="px-3 py-2 text-left font-medium">Last run</th>
                <th className="px-3 py-2 text-left font-medium">Tokens</th>
                <th className="px-3 py-2 text-left font-medium">Last skip</th>
              </tr>
            </thead>
            <tbody>
              {(targets?.results ?? []).map((t) => (
                <tr key={t.id} className="border-t border-subtle">
                  <td className="px-3 py-2">{t.workspace_slug}</td>
                  <td className="px-3 py-2">{t.user_email}</td>
                  <td className="px-3 py-2 text-12 text-secondary">
                    {t.next_run_at ? new Date(t.next_run_at).toLocaleString() : "—"}
                  </td>
                  <td className="px-3 py-2 text-12">{t.last_run ? t.last_run.status : "—"}</td>
                  <td className="px-3 py-2 text-12 text-secondary">{t.last_run?.total_tokens ?? "—"}</td>
                  <td className="px-3 py-2 text-12 text-secondary">{t.last_skip_reason || "—"}</td>
                </tr>
              ))}
              {targets && targets.results.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-4 text-center text-12 text-secondary">
                    No targets yet.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      {editing && (
        <LoopJobFormModal
          job={job}
          onClose={() => setEditing(false)}
          onSaved={() => {
            setEditing(false);
            void mutate();
          }}
        />
      )}
    </PageWrapper>
  );
});

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md border border-subtle bg-surface-1 px-4 py-3">
      <div className="text-h5-semibold text-primary">{value}</div>
      <div className="text-12 text-secondary">{label}</div>
    </div>
  );
}

export const meta = () => [{ title: "Loop Job - God Mode" }];

export default InstanceLoopDetailPage;
