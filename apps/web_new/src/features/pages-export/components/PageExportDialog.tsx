// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog (parity/specs/exports.md §5, EXP-016/017/021): format,
// content scope and (for documents) page size over the live page, saved
// fully client-side. Every opening is a fresh defaulted form; success
// toasts and closes, failure toasts and keeps the choices, cancelling
// downloads nothing. Mounted by the Pages detail through the registration
// seam, never directly in a route.
import { Button, Dialog, FieldScaffold } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { browserPageExportSink, type PageExportSink } from "../lib/downloadSink.js";
import type { PageExportHost } from "../lib/pageExportDialogRegistry.js";
import {
  PAGE_EXPORT_FORMATS,
  PAGE_EXPORT_SCOPES,
  PAGE_EXPORT_SIZES,
  defaultPageExportForm,
  isPageExportSizeVisible,
  type PageExportForm,
  type PageExportFormat,
  type PageExportScope,
  type PageExportSize,
} from "../lib/pageExportModel.js";
import { savePageExport } from "../lib/savePageExport.js";

const DIALOG_TITLE = "Export page";
const SUCCESS_TITLE = "Page exported successfully.";
const FAILURE_TITLE = "Page could not be exported.";
const FAILURE_DESCRIPTION = "Please try again later.";

/** Native picker styling on the kit tokens, with the visible focus ring. */
const SELECT_CLASSES =
  "h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-body text-(--text) disabled:cursor-not-allowed disabled:opacity-50 focus:border-(--accent) focus:outline-none focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-(--focus-ring-color) focus-visible:outline-offset-1";

export interface PageExportDialogProps {
  host: PageExportHost;
  open: boolean;
  onClose(): void;
  sink?: PageExportSink | undefined;
  saveFile?: typeof savePageExport | undefined;
}

export function PageExportDialog({
  host,
  open,
  onClose,
  sink,
  saveFile = savePageExport,
}: PageExportDialogProps): React.ReactElement {
  const [form, setForm] = React.useState<PageExportForm>(defaultPageExportForm);
  const [saving, setSaving] = React.useState(false);

  // Every opening is a fresh defaulted form.
  React.useEffect(() => {
    if (!open) return;
    setForm(defaultPageExportForm());
    setSaving(false);
  }, [open]);

  const handleExport = async (): Promise<void> => {
    setSaving(true);
    try {
      await saveFile(host, form, { sink: sink ?? browserPageExportSink(), origin: window.location.origin });
      notifySuccess(SUCCESS_TITLE);
      onClose();
    } catch {
      notifyError(FAILURE_TITLE, FAILURE_DESCRIPTION);
    } finally {
      setSaving(false);
    }
  };

  const sizeVisible = isPageExportSizeVisible(form.format);

  return (
    <Dialog
      title={DIALOG_TITLE}
      open={open}
      onOpenChange={(next) => {
        if (!next && !saving) onClose();
      }}
      actions={
        <>
          <Button variant="secondary" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button variant="primary" loading={saving} onClick={handleExport}>
            {saving ? "Exporting" : "Export"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-6)">
        <FieldScaffold label="Format" fieldId="page-export-format">
          {(describedBy) => (
            <select
              id="page-export-format"
              aria-describedby={describedBy}
              className={SELECT_CLASSES}
              disabled={saving}
              value={form.format}
              onChange={(event) => setForm({ ...form, format: event.target.value as PageExportFormat })}
            >
              {PAGE_EXPORT_FORMATS.map((format) => (
                <option key={format} value={format}>
                  {format}
                </option>
              ))}
            </select>
          )}
        </FieldScaffold>
        <FieldScaffold label="Content scope" fieldId="page-export-scope">
          {(describedBy) => (
            <select
              id="page-export-scope"
              aria-describedby={describedBy}
              className={SELECT_CLASSES}
              disabled={saving}
              value={form.scope}
              onChange={(event) => setForm({ ...form, scope: event.target.value as PageExportScope })}
            >
              {PAGE_EXPORT_SCOPES.map((scope) => (
                <option key={scope} value={scope}>
                  {scope}
                </option>
              ))}
            </select>
          )}
        </FieldScaffold>
        {sizeVisible ? (
          <FieldScaffold label="Page size" fieldId="page-export-size">
            {(describedBy) => (
              <select
                id="page-export-size"
                aria-describedby={describedBy}
                className={SELECT_CLASSES}
                disabled={saving}
                value={form.size}
                onChange={(event) => setForm({ ...form, size: event.target.value as PageExportSize })}
              >
                {PAGE_EXPORT_SIZES.map((size) => (
                  <option key={size} value={size}>
                    {size}
                  </option>
                ))}
              </select>
            )}
          </FieldScaffold>
        ) : null}
      </div>
    </Dialog>
  );
}
