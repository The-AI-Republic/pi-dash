// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud AI Assistant settings tab (parity/specs/cloud.md §7, CLOUD-032–034,
// 036). The connections manager is the page's first half; the shared
// tool-server registry and the Apps pointer stay reachable below it because
// the edition replaces the whole assistant tab. The list view owns the add
// view behind one header action, and every write reports its outcome here
// so a control that unmounts mid-request cannot swallow it.

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { actionFailureDetail, useCloudAssistantActions } from "../api/mutations.js";
import { useCloudConnections } from "../api/queries.js";
import {
  ADD_ACTION,
  ADD_FAILED_FALLBACK,
  ADD_FAILED_TITLE,
  ADDED_TITLE,
  BACK_ACTION,
  DISCONNECTED_BODY,
  DISCONNECTED_TITLE,
  DISCONNECT_FAILED_TITLE,
  EMPTY_BODY,
  EMPTY_TITLE,
  MODEL_FAILED_TITLE,
  REMOVED_TITLE,
  REMOVE_FAILED_TITLE,
  SWITCHED_TITLE,
  SWITCH_FAILED_TITLE,
  TEST_FAILED_FALLBACK,
  TEST_FAILED_TITLE,
  TEST_OK_BODY,
  TEST_OK_TITLE,
} from "../lib/copy.js";
import { byokSubmission, type ByokDraft } from "../lib/byok.js";
import { connectionModelFlags, connectionTitle, toConnectionRow } from "../lib/rows.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/index.js";
import { AddConnectionView } from "./AddConnectionView.js";
import { AppsPointer } from "./AppsPointer.js";
import { ConnectionsList } from "./ConnectionsList.js";
import { ToolServerSection } from "./ToolServerSection.js";
import { useProviderSignIn } from "./useProviderSignIn.js";

type View = "list" | "add";

export function CloudAssistantTab(): React.ReactElement {
  const connections = useCloudConnections();
  const actions = useCloudAssistantActions();
  const [view, setView] = React.useState<View>("list");
  const [busyId, setBusyId] = React.useState<string | null>(null);
  const [testing, setTesting] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [addError, setAddError] = React.useState<string | null>(null);

  const signIn = useProviderSignIn();

  const list = connections.data;
  const rows =
    list === undefined ? [] : list.connections.map((row) => toConnectionRow(row, list, connectionModelFlags(list)));
  const signInAvailable = list?.openai_login_available ?? false;

  const selectRow = async (id: string): Promise<void> => {
    setBusyId(id);
    try {
      await actions.select(id);
      await actions.invalidate();
      notifySuccess(SWITCHED_TITLE);
    } catch (error) {
      notifyError(SWITCH_FAILED_TITLE, actionFailureDetail(error, SWITCH_FAILED_TITLE));
    } finally {
      setBusyId(null);
    }
  };

  const commitModel = async (id: string, model: string): Promise<void> => {
    setBusyId(id);
    try {
      await actions.setModel(id, model);
      await actions.invalidate();
    } catch (error) {
      notifyError(MODEL_FAILED_TITLE, actionFailureDetail(error, MODEL_FAILED_TITLE));
    } finally {
      setBusyId(null);
    }
  };

  const removeRow = async (id: string, title: string): Promise<void> => {
    setBusyId(id);
    try {
      await actions.remove(id);
      await actions.invalidate();
      notifyInfo(REMOVED_TITLE, `${title} removed.`);
    } catch (error) {
      notifyError(REMOVE_FAILED_TITLE, actionFailureDetail(error, REMOVE_FAILED_TITLE));
    } finally {
      setBusyId(null);
    }
  };

  const disconnectRow = async (id: string): Promise<void> => {
    setBusyId(id);
    try {
      await actions.disconnect(id);
      await actions.invalidate();
      notifyInfo(DISCONNECTED_TITLE, DISCONNECTED_BODY);
    } catch (error) {
      notifyError(DISCONNECT_FAILED_TITLE, actionFailureDetail(error, DISCONNECT_FAILED_TITLE));
    } finally {
      setBusyId(null);
    }
  };

  const testActive = async (): Promise<void> => {
    setTesting(true);
    try {
      const result = await actions.test();
      if (result.ok) {
        notifySuccess(TEST_OK_TITLE, TEST_OK_BODY);
      } else {
        notifyError(TEST_FAILED_TITLE, result.detail ?? result.error_code ?? TEST_FAILED_FALLBACK);
      }
      await actions.invalidate();
    } catch (error) {
      notifyError(TEST_FAILED_TITLE, actionFailureDetail(error, TEST_FAILED_FALLBACK));
    } finally {
      setTesting(false);
    }
  };

  const saveConnection = async (draft: ByokDraft): Promise<void> => {
    setSaving(true);
    setAddError(null);
    try {
      const submission = byokSubmission(draft);
      await actions.create({
        providerKind: submission.providerKind,
        modelName: submission.modelName,
        apiKey: submission.apiKey,
        ...(submission.baseUrl === null ? {} : { baseUrl: submission.baseUrl }),
      });
      await actions.invalidate();
      notifySuccess(ADDED_TITLE);
      setView("list");
    } catch (error) {
      setAddError(actionFailureDetail(error, ADD_FAILED_FALLBACK));
      notifyError(ADD_FAILED_TITLE, actionFailureDetail(error, ADD_FAILED_FALLBACK));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex max-w-2xl flex-col gap-(--space-6)">
      <header className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <h1 className="text-h2 text-(--text)">AI Assistant</h1>
        {view === "list" ? (
          <Button variant="primary" onClick={() => setView("add")}>
            {ADD_ACTION}
          </Button>
        ) : (
          <Button variant="secondary" onClick={() => setView("list")}>
            {BACK_ACTION}
          </Button>
        )}
      </header>

      {view === "add" ? (
        <AddConnectionView saving={saving} error={addError} onSave={saveConnection} />
      ) : connections.isPending ? (
        <Spinner label="Loading connections" />
      ) : connections.isError ? (
        <p role="alert" className="text-body text-(--danger)">
          The connections could not be loaded.
        </p>
      ) : rows.length === 0 ? (
        <div className="flex flex-col items-start gap-(--space-3) rounded-(--radius-control) border border-(--border) bg-(--surface) p-(--space-5)">
          <p className="text-body text-(--text)">
            <strong className="font-medium">{EMPTY_TITLE}.</strong> {EMPTY_BODY}
          </p>
          <Button variant="primary" onClick={() => setView("add")}>
            {ADD_ACTION}
          </Button>
        </div>
      ) : (
        <ConnectionsList
          rows={rows}
          signInAvailable={signInAvailable}
          signInPending={signIn.pending}
          testing={testing}
          busyId={busyId}
          onSelect={(row) => void selectRow(row.id)}
          onCommitModel={(row, model) => void commitModel(row.id, model)}
          onTest={() => void testActive()}
          onRemove={(row) => void removeRow(row.id, row.title)}
          onDisconnect={(row) => void disconnectRow(row.id)}
          onSignIn={() => void signIn.signIn()}
        />
      )}

      {view === "list" ? (
        <>
          <hr className="border-(--border)" />
          <ToolServerSection />
          <AppsPointer />
        </>
      ) : null}
    </div>
  );
}

/** The row title helper is also used by tests to pin the naming rule. */
export { connectionTitle };
