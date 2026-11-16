// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attachments widget (parity/specs/issues.md §15, ISS-187–191). Rows are
// the uploaded assets: a kind icon, the stored name, the human size and
// the uploader; the whole row links to the served asset URL, which the
// browser opens in a new tab. Members upload from a hidden picker or by
// dropping one file anywhere over the section; the instance ceiling is
// enforced before the grant is requested. Deletes confirm first. The
// section renders only once a row exists; an empty issue is attached
// from the widget root's action row, which opens this picker.

import { Avatar, Button, Dialog, IconButton, Menu, Spinner, Tooltip } from "@pidash/kit";
import type { IssueAttachment } from "@pidash/api-client";
import { Ellipsis, File, FileArchive, FileImage, FileSpreadsheet, FileText, Paperclip, Upload } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { useInstanceConfig } from "../../../../../core/session/index.js";
import { usePickerWorkspaceMembers } from "../../../../../shared/pickers/index.js";
import { notifyError, resolveFileUrl } from "../../../../../shared/shell/index.js";
import { timeAgo } from "../../../../../shared/lib/timeAgo.js";
import { useDeleteAttachment, useIssueAttachments, useUploadAttachment } from "./api.js";
import { uploadAttachmentBytes } from "./uploadBytes.js";
import {
  attachmentBaseName,
  attachmentName,
  attachmentSizeLimit,
  attachmentKind,
  exceedsSizeLimit,
  formatAttachmentSize,
  MULTI_FILE_NOTICE,
  oneFileOf,
  oversizeNotice,
  type AttachmentKind,
} from "./format.js";

export interface AttachmentsWidgetHandle {
  /** Opens the hidden file picker, shared with the widget root's Attach entry. */
  openPicker: () => void;
}

export interface AttachmentsWidgetProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Guests, archived and synced items read; no attach or delete. */
  editable: boolean;
}

const KIND_ICONS: Record<AttachmentKind, typeof File> = {
  image: FileImage,
  pdf: FileText,
  document: FileText,
  sheet: FileSpreadsheet,
  archive: FileArchive,
  file: File,
};

/** Progress re-renders at most this often, so a fast upload stays readable. */
export const UPLOAD_PROGRESS_THROTTLE_MS = 200;

/** Icon for one attachment's extension family. */
function AttachmentIcon({ name }: { name: string }): React.ReactElement {
  const Icon = KIND_ICONS[attachmentKind(name)];
  return <Icon size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />;
}

/** Confirmation for the attachment the row menu selected. */
function DeleteAttachmentDialog({
  attachment,
  busy,
  onCancel,
  onConfirm,
}: {
  attachment: IssueAttachment;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (attachment: IssueAttachment) => void;
}): React.ReactElement {
  return (
    <Dialog
      title="Delete attachment"
      open
      onOpenChange={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="danger" loading={busy} onClick={() => onConfirm(attachment)}>
            Delete
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">
        {`The file ${attachmentBaseName(attachmentName(attachment))} will be permanently removed from this work item.`}
      </p>
    </Dialog>
  );
}

export const AttachmentsWidget = React.forwardRef<AttachmentsWidgetHandle, AttachmentsWidgetProps>(
  function AttachmentsWidget({ workspaceSlug, projectId, issueId, editable }, ref): React.ReactElement {
    const query = useIssueAttachments(workspaceSlug, projectId, issueId);
    const upload = useUploadAttachment(workspaceSlug, projectId, issueId);
    const remove = useDeleteAttachment(workspaceSlug, projectId, issueId);
    const instanceConfig = useInstanceConfig();
    const directory = usePickerWorkspaceMembers(workspaceSlug, true);
    const limit = attachmentSizeLimit(instanceConfig.data?.file_size_limit);
    const [expanded, setExpanded] = React.useState(true);
    const [dragging, setDragging] = React.useState(false);
    const [pendingName, setPendingName] = React.useState<string | null>(null);
    const [percent, setPercent] = React.useState(0);
    const [confirming, setConfirming] = React.useState<IssueAttachment | null>(null);
    const inputRef = React.useRef<HTMLInputElement | null>(null);
    const progressClock = React.useRef(0);

    const rows = query.data ?? [];
    const members = new Map((directory.data ?? []).map((row) => [row.member.id, row.member]));

    function acceptFiles(files: readonly File[]): void {
      if (pendingName !== null) return;
      const file = oneFileOf(files);
      if (file === null) {
        if (files.length > 0) notifyError(MULTI_FILE_NOTICE);
        return;
      }
      if (exceedsSizeLimit(file.size, limit)) {
        notifyError(oversizeNotice(limit));
        return;
      }
      progressClock.current = 0;
      setPercent(0);
      setPendingName(file.name);
      upload.mutate(
        {
          file,
          uploadBytes: uploadAttachmentBytes((percent) => {
            const now = Date.now();
            if (percent < 100 && now - progressClock.current < UPLOAD_PROGRESS_THROTTLE_MS) return;
            progressClock.current = now;
            setPercent(percent);
          }),
        },
        {
          onSettled: () => {
            setPendingName(null);
            if (inputRef.current !== null) inputRef.current.value = "";
          },
        }
      );
    }

    function openPicker(): void {
      if (pendingName !== null) return;
      inputRef.current?.click();
    }

    React.useImperativeHandle(ref, () => ({ openPicker }));

    const dragHandlers = editable
      ? {
          onDragEnter: (event: React.DragEvent) => {
            event.preventDefault();
            if (pendingName === null) setDragging(true);
          },
          onDragOver: (event: React.DragEvent) => {
            event.preventDefault();
          },
          onDragLeave: (event: React.DragEvent) => {
            if (event.currentTarget === event.target) setDragging(false);
          },
          onDrop: (event: React.DragEvent) => {
            event.preventDefault();
            setDragging(false);
            acceptFiles(Array.from(event.dataTransfer.files));
          },
        }
      : {};

    return (
      <section
        data-widget-root="Attachments"
        data-file-size-limit={limit}
        aria-label="Attachments"
        className={`relative flex flex-col gap-(--space-2) rounded-(--radius-control) ${
          dragging ? "bg-(--subtle) outline-2 outline-(--accent) outline-dashed" : ""
        }`}
        {...dragHandlers}
      >
        {editable ? (
          <input
            ref={inputRef}
            type="file"
            className="sr-only"
            aria-label="Choose an attachment"
            onChange={(event) => acceptFiles(Array.from(event.target.files as FileList))}
          />
        ) : null}
        {rows.length === 0 && pendingName === null ? null : (
          <>
            <button
              type="button"
              data-widget-header="Attachments"
              aria-expanded={expanded}
              onClick={() => setExpanded(!expanded)}
              className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) pr-24 text-left font-medium text-(--text) hover:bg-(--subtle)"
            >
              <Paperclip size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
              <span className="truncate">Attachments</span>
              <span data-widget-count className="text-meta text-(--text-muted)">
                {rows.length}
              </span>
            </button>
            {editable ? (
              <Button
                size="small"
                variant="ghost"
                onClick={openPicker}
                loading={pendingName !== null}
                disabled={pendingName !== null}
                className="absolute top-0 right-0"
              >
                <Upload size={14} strokeWidth={1.5} aria-hidden="true" />
                Attach
              </Button>
            ) : null}
            {expanded ? (
              <div className="grid overflow-hidden" data-widget-rows="Attachments">
                {pendingName === null ? null : (
                  <div
                    data-widget-row="Attachments"
                    className="flex items-center gap-(--space-3) px-(--space-2) py-(--space-2)"
                  >
                    <AttachmentIcon name={pendingName} />
                    <span className="text-body min-w-0 flex-1 truncate text-(--text)">{pendingName}</span>
                    <span data-widget-progress className="text-meta shrink-0 text-(--text-muted)">
                      {`${percent}%`}
                    </span>
                    <Spinner size="small" label="Uploading attachment" />
                  </div>
                )}
                {rows.map((row) => {
                  const name = attachmentName(row);
                  const url = resolveFileUrl(row.asset_url ?? row.asset, getAppClient().baseUrl);
                  const uploader = row.user === null ? undefined : members.get(row.user);
                  return (
                    <div
                      key={row.id}
                      data-widget-row="Attachments"
                      className="group/row flex items-center gap-(--space-3) rounded-(--radius-control) px-(--space-2) py-(--space-2) hover:bg-(--subtle)"
                    >
                      {url === null ? (
                        <div className="flex min-w-0 flex-1 items-center gap-(--space-3)">
                          <AttachmentIcon name={name} />
                          <span className="text-body min-w-0 flex-1 truncate text-(--text)">{name}</span>
                          <span className="text-meta shrink-0 text-(--text-muted)">
                            {formatAttachmentSize(row.size)}
                          </span>
                        </div>
                      ) : (
                        <a
                          href={url}
                          target="_blank"
                          rel="noreferrer noopener"
                          className="flex min-w-0 flex-1 items-center gap-(--space-3) text-(--text) hover:underline"
                        >
                          <AttachmentIcon name={name} />
                          <span className="text-body min-w-0 flex-1 truncate">{name}</span>
                          <span className="text-meta shrink-0 text-(--text-muted)">
                            {formatAttachmentSize(row.size)}
                          </span>
                        </a>
                      )}
                      {uploader === undefined ? null : (
                        <Tooltip content={`Uploaded ${timeAgo(row.created_at)}`}>
                          <span className="shrink-0">
                            <Avatar
                              name={uploader.display_name}
                              size="small"
                              {...(uploader.avatar_url === null ? {} : { src: uploader.avatar_url })}
                            />
                          </span>
                        </Tooltip>
                      )}
                      {editable ? (
                        <span className="shrink-0">
                          <Menu
                            trigger={
                              <IconButton
                                label={`Actions for ${name}`}
                                data-widget-row-menu="Attachments"
                                size="small"
                                className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
                              >
                                <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
                              </IconButton>
                            }
                            items={[
                              {
                                label: "Delete",
                                onSelect: () => setConfirming(row),
                              },
                            ]}
                          />
                        </span>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            ) : null}
          </>
        )}
        {confirming === null ? null : (
          <DeleteAttachmentDialog
            attachment={confirming}
            busy={remove.isPending}
            onCancel={() => setConfirming(null)}
            onConfirm={(attachment) => {
              remove.mutate(attachment.id, {
                onSettled: () => setConfirming(null),
              });
            }}
          />
        )}
      </section>
    );
  }
);
