// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run detail pane (NEWFRONT-545, RUN-017–021). One pane for every run:
// identity, status and executor, the scheduler back-link, the full prompt,
// failure diagnostics, result, the cloud-execution panel, the event
// transcript and the cancel control. Data arrives from the parent hook so
// the pane itself stays pure and testable.

import { Badge, Button, Dialog, Spinner } from "@pidash/kit";
import * as React from "react";

import type { AgentRun } from "@pidash/api-client";
import { isTerminalRun } from "../api/runs.js";
import {
  errorKindLabel,
  errorSourceTone,
  eventNarrative,
  eventTime,
  executorLabel,
  executorTone,
  hasFailure,
  resultJson,
  resultSummary,
} from "./runsModel.js";
import { RunStatusBadge } from "./RunStatusBadge.js";

export const RUN_DETAIL_NONE_TEXT = "Select a run on the left.";
export const RUN_DETAIL_LOADING_TEXT = "Loading run details…";
export const RUN_DETAIL_UNAVAILABLE_TEXT =
  "This run is not available. It may have been deleted or belong to a different workspace.";
export const RUN_CANCEL_TITLE = "Cancel run?";
export const RUN_CANCEL_CLOUD_BODY =
  "The cloud agent stops at its next safe interruption point, so it may take a moment to finish.";
export const RUN_CANCEL_LOCAL_BODY =
  "The runner receives a stop request and ends the run as soon as it can acknowledge it.";
export const RUN_CANCEL_FAILED_TITLE = "Couldn't cancel run";
export const RUN_CANCEL_FAILED = "The run is still active. Try again.";

/** Cancelling makes sense until the run settles or a request already exists. */
export function canCancelRun(run: Pick<AgentRun, "status">): boolean {
  return !isTerminalRun(run.status) && run.status !== "cancel_requested";
}

/** Executor-specific consequence copy for the confirmation. */
export function cancelBodyKind(kind: AgentRun["executor_kind"]): string {
  return kind === "cloud_agent" ? RUN_CANCEL_CLOUD_BODY : RUN_CANCEL_LOCAL_BODY;
}

export interface RunDetailProps {
  state: "none" | "loading" | "loaded" | "unavailable";
  run: AgentRun | null;
  cancelling: boolean;
  /** Called with the run; resolves true once the server accepts the cancel. */
  onCancel: (run: AgentRun) => Promise<boolean>;
  /** Scheduler base path for the back-link; null hides the row. */
  schedulerHref: (run: AgentRun) => string | null;
}

export function RunDetail({ state, run, cancelling, onCancel, schedulerHref }: RunDetailProps): React.ReactElement {
  const [cancelOpen, setCancelOpen] = React.useState(false);
  if (state === "none") {
    return <p className="text-body text-(--text-muted)">{RUN_DETAIL_NONE_TEXT}</p>;
  }
  if (state === "unavailable") {
    return <p className="text-body text-(--danger)">{RUN_DETAIL_UNAVAILABLE_TEXT}</p>;
  }
  if (state === "loading" || run === null) {
    return (
      <p className="text-body flex items-center gap-(--space-2) text-(--text-muted)">
        <Spinner size="small" />
        {RUN_DETAIL_LOADING_TEXT}
      </p>
    );
  }
  const scheduler = schedulerHref(run);
  const summary = resultSummary(run);
  return (
    <div className="flex flex-col gap-(--space-4)" data-testid="run-detail">
      <header className="flex items-start justify-between gap-(--space-4)">
        <div>
          <div className="font-mono text-meta text-(--text-muted)" data-testid="run-detail-id">
            {run.id}
          </div>
          <div className="mt-(--space-1) flex flex-wrap items-center gap-(--space-2)">
            <span data-testid="run-status">
              <RunStatusBadge status={run.status} />
            </span>
            <span data-testid="run-executor">
              <Badge tone={executorTone(run.executor_kind)}>{executorLabel(run.executor_kind)}</Badge>
            </span>
          </div>
        </div>
        {canCancelRun(run) ? (
          <Button variant="danger" size="small" data-testid="run-cancel" onClick={() => setCancelOpen(true)}>
            Cancel run
          </Button>
        ) : null}
      </header>

      {scheduler === null ? null : (
        <p className="text-body">
          <span className="text-(--text-muted)">Scheduler: </span>
          <a href={scheduler} data-testid="run-scheduler-link" className="font-medium text-(--accent) hover:underline">
            {run.scheduler_binding_detail?.scheduler_name ?? ""}
          </a>
        </p>
      )}

      <section aria-label="Prompt">
        <h2 className="text-meta text-(--text-muted)">Prompt</h2>
        <pre
          data-testid="run-prompt"
          className="text-body mt-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text)"
        >
          {run.prompt}
        </pre>
      </section>

      {hasFailure(run) ? (
        <section aria-label="Error">
          <h2 className="text-meta text-(--danger)">Error</h2>
          {run.error_diagnostic === null ? null : (
            <div
              data-testid="run-error-diagnostic"
              className="mt-(--space-1) flex flex-col gap-(--space-1) rounded-(--radius-control) border border-(--warning) p-(--space-3)"
            >
              <div className="flex flex-wrap items-center gap-(--space-2)">
                <span className="text-meta text-(--text-muted)">Failure source</span>
                <span data-testid="run-error-source">
                  <Badge tone={errorSourceTone(run.error_diagnostic.source)}>{run.error_diagnostic.source_label}</Badge>
                </span>
                <span className="text-meta text-(--text-muted)" data-testid="run-error-kind">
                  {errorKindLabel(run.error_diagnostic.kind)}
                </span>
              </div>
              <p className="text-body text-(--text)" data-testid="run-error-summary">
                {run.error_diagnostic.summary}
              </p>
              {run.error_diagnostic.action === "" ? null : (
                <p className="text-meta text-(--text-muted)" data-testid="run-error-action">
                  {run.error_diagnostic.action}
                </p>
              )}
            </div>
          )}
          <pre
            data-testid="run-error"
            className="text-meta mt-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--danger)"
          >
            {run.error}
          </pre>
        </section>
      ) : null}

      {run.done_payload === null ? null : (
        <section aria-label="Result">
          <h2 className="text-meta text-(--text-muted)">Result</h2>
          {summary === null ? null : (
            <div
              data-testid="run-result-summary"
              className="text-body mt-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-3) whitespace-pre-wrap"
            >
              {summary}
            </div>
          )}
          <pre
            data-testid="run-result"
            className="text-meta mt-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap"
          >
            {resultJson(run.done_payload)}
          </pre>
        </section>
      )}

      {run.executor_kind === "cloud_agent" ? (
        <section
          aria-label="Cloud execution"
          data-testid="run-cloud-panel"
          className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
        >
          <h2 className="text-emphasis text-(--text)">Cloud execution</h2>
          <p className="text-meta text-(--text-muted)">
            A cloud agent runs without a development machine. It can only use the tools shown here and has no filesystem
            or shell access.
          </p>
          <div className="flex flex-wrap gap-(--space-1)" data-testid="run-cloud-tools">
            {(run.tool_plan.tools ?? []).map((tool) => (
              <span key={tool} data-testid="run-cloud-tool">
                <Badge tone="neutral">{tool}</Badge>
              </span>
            ))}
          </div>
          {run.tool_calls.length === 0 ? null : (
            <ul className="flex flex-col gap-(--space-1)" data-testid="run-cloud-calls">
              {run.tool_calls.map((call) => (
                <li
                  key={call.id}
                  data-testid="run-cloud-call"
                  className="text-meta flex items-center justify-between gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)"
                >
                  <span className="font-mono" data-testid="run-cloud-call-name">
                    {call.tool_name}
                  </span>
                  <span data-testid="run-cloud-call-meta">
                    {call.risk} · {call.status}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : null}

      <section aria-label="Events">
        <h2 className="text-meta text-(--text-muted)">Events ({run.events?.length ?? 0})</h2>
        <div className="mt-(--space-1) max-h-96 overflow-auto rounded-(--radius-control) border border-(--border)">
          <table data-testid="run-events" className="text-meta w-full text-left">
            <thead className="bg-(--subtle) text-(--text-muted)">
              <tr>
                <th scope="col" className="px-(--space-2) py-(--space-1) font-medium">
                  seq
                </th>
                <th scope="col" className="px-(--space-2) py-(--space-1) font-medium">
                  kind
                </th>
                <th scope="col" className="px-(--space-2) py-(--space-1) font-medium">
                  at
                </th>
              </tr>
            </thead>
            <tbody>
              {(run.events ?? []).map((event) => {
                const narrative = eventNarrative(event);
                return (
                  <React.Fragment key={event.id}>
                    <tr data-testid="run-event-row" className="border-t border-(--border)">
                      <td className="font-mono px-(--space-2) py-(--space-1)">{event.seq}</td>
                      <td className="font-mono px-(--space-2) py-(--space-1)">{event.kind}</td>
                      <td className="px-(--space-2) py-(--space-1) whitespace-nowrap">{eventTime(event.created_at)}</td>
                    </tr>
                    {narrative === null ? null : (
                      <tr data-testid="run-event-narrative-row">
                        <td />
                        <td colSpan={2} className="px-(--space-2) pt-0 pb-(--space-2)">
                          <pre
                            data-testid="run-event-narrative"
                            className="text-body m-0 rounded-(--radius-control) bg-(--subtle) p-(--space-2) whitespace-pre-wrap text-(--text)"
                          >
                            {narrative}
                          </pre>
                        </td>
                      </tr>
                    )}
                  </React.Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <Dialog
        open={cancelOpen}
        onOpenChange={(next) => {
          if (!next && !cancelling) setCancelOpen(false);
        }}
        title={RUN_CANCEL_TITLE}
        nonDismissable={cancelling}
        bodyClassName="mt-(--space-2)"
        actions={
          <>
            <Button
              variant="ghost"
              aria-disabled={cancelling || undefined}
              onClick={() => !cancelling && setCancelOpen(false)}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              loading={cancelling}
              data-testid="run-cancel-confirm"
              onClick={() => {
                if (!cancelling) {
                  void onCancel(run).then((accepted) => {
                    if (accepted) setCancelOpen(false);
                  });
                }
              }}
            >
              Cancel run
            </Button>
          </>
        }
      >
        <div data-testid="run-cancel-body">
          <p className="text-body text-(--text-muted)">{cancelBodyKind(run.executor_kind)}</p>
        </div>
      </Dialog>
    </div>
  );
}
