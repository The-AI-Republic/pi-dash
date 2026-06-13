/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { observer } from "mobx-react";
import { Link } from "react-router";
import useSWR from "swr";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { InstanceLoopService } from "@pi-dash/services";
import type { ILoopJob } from "@pi-dash/types";
import { Button, Loader, ToggleSwitch } from "@pi-dash/ui";
// components
import { PageWrapper } from "@/components/common/page-wrapper";
// local
import { LoopJobFormModal } from "./form-modal";

const service = new InstanceLoopService();

const InstanceLoopPage = observer(function InstanceLoopPage() {
  const { data: jobs, mutate, isLoading } = useSWR<ILoopJob[]>("INSTANCE_LOOP_JOBS", () => service.list());
  const [creating, setCreating] = useState(false);

  const toggleEnabled = async (job: ILoopJob, enabled: boolean) => {
    void mutate((prev) => prev?.map((j) => (j.id === job.id ? { ...j, enabled } : j)), false);
    try {
      await service.update(job.id, { enabled });
      void mutate();
    } catch {
      setToast({ type: TOAST_TYPE.ERROR, title: "Update failed", message: "Could not change the job state." });
      void mutate();
    }
  };

  return (
    <PageWrapper
      header={{
        title: "Loop — Auto Project Management",
        description:
          "Scheduled AI jobs that run as each user to manage their issues. Users see these as “Auto Project Management”.",
        actions: <Button onClick={() => setCreating(true)}>New job</Button>,
      }}
    >
      {isLoading || !jobs ? (
        <Loader className="space-y-4">
          <Loader.Item height="48px" />
          <Loader.Item height="48px" />
        </Loader>
      ) : jobs.length === 0 ? (
        <p className="px-4 text-body-sm-regular text-secondary">No jobs yet. Create one to get started.</p>
      ) : (
        <div className="mx-4 overflow-hidden rounded-md border border-subtle">
          <table className="w-full text-body-sm-regular">
            <thead className="bg-surface-2 text-secondary">
              <tr>
                <th className="px-3 py-2 text-left font-medium">Name</th>
                <th className="px-3 py-2 text-left font-medium">Slug</th>
                <th className="px-3 py-2 text-left font-medium">Recurrence</th>
                <th className="px-3 py-2 text-left font-medium">Min role</th>
                <th className="px-3 py-2 text-left font-medium">Builtin</th>
                <th className="px-3 py-2 text-left font-medium">Enabled</th>
              </tr>
            </thead>
            <tbody>
              {jobs.map((job) => (
                <tr key={job.id} className="border-t border-subtle">
                  <td className="px-3 py-2">
                    <Link to={`/loop/${job.id}`} className="text-primary underline-offset-2 hover:underline">
                      {job.name}
                    </Link>
                  </td>
                  <td className="font-mono px-3 py-2 text-12 text-secondary">{job.slug}</td>
                  <td className="font-mono px-3 py-2 text-12 text-secondary">{job.rrule}</td>
                  <td className="px-3 py-2">{ROLE_LABELS[job.min_role] ?? job.min_role}</td>
                  <td className="px-3 py-2">{job.is_builtin ? "Yes" : "No"}</td>
                  <td className="px-3 py-2">
                    <ToggleSwitch value={job.enabled} onChange={(v) => toggleEnabled(job, v)} size="sm" />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {creating && (
        <LoopJobFormModal
          onClose={() => setCreating(false)}
          onSaved={() => {
            setCreating(false);
            void mutate();
          }}
        />
      )}
    </PageWrapper>
  );
});

export const ROLE_LABELS: Record<number, string> = { 20: "Admin", 15: "Member", 5: "Guest" };

export const meta = () => [{ title: "Loop Settings - God Mode" }];

export default InstanceLoopPage;
