/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { InstanceLoopService } from "@pi-dash/services";
import type { ILoopJob, ILoopJobInput } from "@pi-dash/types";
import { Button, EModalWidth, ModalCore } from "@pi-dash/ui";

const service = new InstanceLoopService();

type Props = {
  job?: ILoopJob;
  onClose: () => void;
  onSaved: (job: ILoopJob) => void;
};

const FIELD = "rounded-md border border-subtle bg-surface-1 px-3 py-2 text-body-sm-regular";

export function LoopJobFormModal({ job, onClose, onSaved }: Props) {
  const editing = !!job;
  const [slug, setSlug] = useState(job?.slug ?? "");
  const [name, setName] = useState(job?.name ?? "");
  const [publicName, setPublicName] = useState(job?.public_name ?? "");
  const [publicDescription, setPublicDescription] = useState(job?.public_description ?? "");
  const [prompt, setPrompt] = useState(job?.prompt ?? "");
  const [minRole, setMinRole] = useState(job?.min_role ?? 15);
  const [rrule, setRrule] = useState(job?.rrule ?? "FREQ=DAILY;BYHOUR=3;BYMINUTE=0");
  const [tzid, setTzid] = useState(job?.tzid ?? "UTC");
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    const payload: ILoopJobInput = {
      slug,
      name,
      public_name: publicName,
      public_description: publicDescription,
      prompt,
      min_role: minRole,
      rrule,
      tzid,
    };
    try {
      const result = editing ? await service.update(job!.id, payload) : await service.create(payload);
      setToast({ type: TOAST_TYPE.SUCCESS, title: "Saved", message: "Loop job saved." });
      onSaved(result);
    } catch (e: unknown) {
      const err = e as { error?: string; detail?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Save failed",
        message: err?.detail || err?.error || "Check the fields and try again.",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <ModalCore isOpen handleClose={onClose} width={EModalWidth.XXL}>
      <div className="flex max-h-[85vh] flex-col gap-3 overflow-y-auto p-5">
        <h3 className="text-h6-semibold text-primary">{editing ? "Edit job" : "New job"}</h3>

        <label className="flex flex-col gap-1 text-12 text-secondary">
          Slug
          <input
            className={FIELD}
            value={slug}
            onChange={(e) => setSlug(e.target.value)}
            placeholder="auto-close-merged"
            disabled={editing && job?.is_builtin}
          />
        </label>
        <label className="flex flex-col gap-1 text-12 text-secondary">
          Admin name
          <input className={FIELD} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-12 text-secondary">
          User-facing name
          <input className={FIELD} value={publicName} onChange={(e) => setPublicName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-12 text-secondary">
          User-facing description
          <textarea
            className={FIELD}
            rows={2}
            value={publicDescription}
            onChange={(e) => setPublicDescription(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1 text-12 text-secondary">
          Prompt
          <textarea
            className={`${FIELD} font-mono`}
            rows={6}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
          />
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-12 text-secondary">
            Min role
            <select className={FIELD} value={minRole} onChange={(e) => setMinRole(Number(e.target.value))}>
              <option value={5}>Guest</option>
              <option value={15}>Member</option>
              <option value={20}>Admin</option>
            </select>
          </label>
          <label className="flex flex-col gap-1 text-12 text-secondary">
            Timezone
            <input className={FIELD} value={tzid} onChange={(e) => setTzid(e.target.value)} />
          </label>
        </div>
        <label className="flex flex-col gap-1 text-12 text-secondary">
          Recurrence (RRULE — hourly or slower)
          <input
            className={`${FIELD} font-mono`}
            value={rrule}
            onChange={(e) => setRrule(e.target.value)}
            placeholder="FREQ=DAILY;BYHOUR=3;BYMINUTE=0"
          />
        </label>

        <div className="mt-2 flex justify-end gap-2">
          <Button variant="neutral-primary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={save}
            loading={saving}
            disabled={!slug.trim() || !name.trim() || !publicName.trim() || !prompt.trim()}
          >
            Save
          </Button>
        </div>
      </div>
    </ModalCore>
  );
}
