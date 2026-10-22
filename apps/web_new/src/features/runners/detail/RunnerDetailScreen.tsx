// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail screen (NEWFRONT-555, RUN-042/RUN-043). One runner's header,
// metadata grid and activity panel, with a back link to the scope's list and
// a control into the runner's chat. Loading and failure replace the body;
// the page is a plain deep-linkable address.

import { Badge } from "@pidash/kit";
import type { Runner } from "@pidash/api-client";
import { Link } from "@tanstack/react-router";
import { ArrowLeft, MessageSquare } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { RUNNER_STATUS_LABELS, RUNNER_STATUS_TONES, runnerDisplayStatus } from "../status.js";
import { ActivityPanel } from "./ActivityPanel.js";

export const RUNNER_DETAIL_TITLE = "Runner detail";
export const RUNNER_DETAIL_LOADING = "Loading…";
export const RUNNER_DETAIL_FAILED = "Failed to load runner";
export const RUNNER_DETAIL_BACK = "Back to runners";
export const RUNNER_DETAIL_CHAT = "Open chat";
export const DASH = "—";

export function runnerDetailDocumentTitle(name: string | undefined): string {
  return name === undefined ? RUNNER_DETAIL_TITLE : `Runner ${name}`;
}

/** OS/arch pair, or a dash when the runner never reported either. */
export function platformLabel(runner: Pick<Runner, "os" | "arch">): string {
  return runner.os === "" && runner.arch === "" ? DASH : `${runner.os} / ${runner.arch}`;
}

export function machineLabel(runner: Pick<Runner, "dev_machine_detail">): string {
  const detail = runner.dev_machine_detail;
  if (detail === null) return DASH;
  if (detail.label !== "") return detail.label;
  return detail.host_label !== "" ? detail.host_label : DASH;
}

/** Locale date with a dash fallback; formatter injectable for tests. */
export function timeLabel(
  value: string | null,
  format: (iso: string) => string = (iso) => new Date(iso).toLocaleString()
): string {
  return value === null ? DASH : format(value);
}

export function enrolledLabel(runner: Pick<Runner, "enrolled_at">, format?: (iso: string) => string): string {
  return runner.enrolled_at === null ? "Pending enrollment" : timeLabel(runner.enrolled_at, format);
}

export interface RunnerDetailScreenProps {
  workspaceSlug: string;
  runnerId: string;
  projectId?: string | undefined;
  runner: Runner | undefined;
  loading: boolean;
  failed: boolean;
}

function MetaRow({ label, children }: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <>
      <dt className="text-(--text-muted)">{label}</dt>
      <dd className="text-(--text)">{children}</dd>
    </>
  );
}

export function RunnerDetailScreen({
  workspaceSlug,
  runnerId,
  projectId,
  runner,
  loading,
  failed,
}: RunnerDetailScreenProps): React.ReactElement {
  useDocumentHead({ title: runnerDetailDocumentTitle(runner?.name), robots: "noindex, nofollow" });
  const base =
    projectId === undefined ? `/${workspaceSlug}/runners` : `/${workspaceSlug}/projects/${projectId}/runners`;
  const chatHref = `${base}/chat/${runnerId}`;
  const showBody = failed || (loading && runner === undefined);
  return (
    <div className="flex flex-col gap-(--space-6) p-(--space-6)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <Link
          to={base}
          className="text-body inline-flex items-center gap-(--space-1) text-(--text-muted) hover:text-(--text)"
        >
          <ArrowLeft size={16} aria-hidden="true" />
          <span>{RUNNER_DETAIL_BACK}</span>
        </Link>
        {runner === undefined ? null : (
          <Link
            to={chatHref}
            className="text-body inline-flex h-(--control-height) items-center gap-(--space-2) rounded-(--radius-control) bg-(--accent) px-(--space-4) font-medium text-(--on-accent)"
          >
            <MessageSquare size={16} aria-hidden="true" />
            {RUNNER_DETAIL_CHAT}
          </Link>
        )}
      </div>

      {failed ? (
        <div
          className="text-body rounded-(--radius-control) border border-(--border) p-(--space-4) text-(--danger)"
          role="alert"
        >
          {RUNNER_DETAIL_FAILED}
        </div>
      ) : showBody ? (
        <div
          className="text-body rounded-(--radius-control) border border-(--border) p-(--space-4) text-(--text-muted)"
          role="status"
        >
          {RUNNER_DETAIL_LOADING}
        </div>
      ) : runner === undefined ? null : (
        <>
          <section className="rounded-(--radius-control) border border-(--border) p-(--space-4)">
            <div className="flex items-center gap-(--space-3)">
              <h1 className="text-emphasis font-mono font-semibold break-all text-(--text)">{runner.name}</h1>
              <button type="button" tabIndex={-1} className="contents cursor-default">
                <Badge tone={RUNNER_STATUS_TONES[runnerDisplayStatus(runner)]} dot>
                  {RUNNER_STATUS_LABELS[runnerDisplayStatus(runner)]}
                </Badge>
              </button>
            </div>
          </section>

          <section className="rounded-(--radius-control) border border-(--border) p-(--space-4)">
            <h2 className="text-emphasis mb-(--space-3) font-medium text-(--text)">Metadata</h2>
            <dl className="text-body grid grid-cols-[max-content_1fr] gap-x-(--space-6) gap-y-(--space-2)">
              <MetaRow label="Runner ID">
                <span className="font-mono text-meta">{runner.id}</span>
              </MetaRow>
              <MetaRow label="Pod">{runner.pod_detail.name}</MetaRow>
              <MetaRow label="Project">{runner.pod_detail.project_identifier}</MetaRow>
              <MetaRow label="Dev machine">{machineLabel(runner)}</MetaRow>
              <MetaRow label="OS / Arch">{platformLabel(runner)}</MetaRow>
              <MetaRow label="Version">{runner.runner_version === "" ? DASH : runner.runner_version}</MetaRow>
              <MetaRow label="Working directory">
                {runner.dev_metadata.working_dir === undefined || runner.dev_metadata.working_dir === "" ? (
                  DASH
                ) : (
                  <span className="font-mono text-meta break-all">{runner.dev_metadata.working_dir}</span>
                )}
              </MetaRow>
              <MetaRow label="Protocol version">{runner.protocol_version}</MetaRow>
              <MetaRow label="Capabilities">
                {runner.capabilities.length === 0 ? (
                  DASH
                ) : (
                  <span className="flex flex-wrap gap-(--space-1)">
                    {runner.capabilities.map((capability) => (
                      <Badge key={capability} tone="neutral">
                        {capability}
                      </Badge>
                    ))}
                  </span>
                )}
              </MetaRow>
              <MetaRow label="Connection">{DASH}</MetaRow>
              <MetaRow label="Owner">
                <span className="font-mono text-meta">{runner.owner}</span>
              </MetaRow>
              <MetaRow label="Last heartbeat">{timeLabel(runner.last_heartbeat_at)}</MetaRow>
              <MetaRow label="Enrolled at">{enrolledLabel(runner)}</MetaRow>
              {runner.revoked_at === null ? null : <MetaRow label="Revoked at">{timeLabel(runner.revoked_at)}</MetaRow>}
              {runner.revoked_reason === "" ? null : <MetaRow label="Revoked reason">{runner.revoked_reason}</MetaRow>}
              <MetaRow label="Created at">{timeLabel(runner.created_at)}</MetaRow>
              <MetaRow label="Updated at">{timeLabel(runner.updated_at)}</MetaRow>
            </dl>
          </section>

          <ActivityPanel runnerName={runner.name} liveState={runner.live_state} />
        </>
      )}
    </div>
  );
}
