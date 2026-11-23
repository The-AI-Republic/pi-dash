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
import { useParams } from "@tanstack/react-router";
import * as React from "react";

import { actionFailureDetail, useCloudAssistantActions } from "../api/mutations.js";
import { useCloudConnections, useCloudQuota } from "../api/queries.js";
import { modelTestResultsFrom, type ModelTestResult, type ModelTestResults } from "@pidash/api-client";
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
  MODEL_BLOCKED_BODY,
  MODEL_BLOCKED_TITLE,
  MODEL_FAILED_TITLE,
  REMOVED_TITLE,
  REMOVE_FAILED_TITLE,
  TEST_FAILED_FALLBACK,
  TEST_FAILED_TITLE,
  TEST_OK_BODY,
  TEST_OK_TITLE,
} from "../lib/copy.js";
import { byokSubmission, type ByokDraft } from "../lib/byok.js";
import {
  connectionModelFlags,
  connectionTitle,
  isBlockedCommit,
  toConnectionRow,
  type ConnectionRow,
} from "../lib/rows.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/index.js";
import { AddConnectionView } from "./AddConnectionView.js";
import { AppsPointer } from "./AppsPointer.js";
import { AssistantModelsSection } from "./AssistantModelsSection.js";
import { ConnectionsList } from "./ConnectionsList.js";
import { FreeTokenMeter } from "./FreeTokenMeter.js";
import { ToolServersSection } from "../../profile-assistant/index.js";
import { useProviderSignIn } from "./useProviderSignIn.js";

type View = "list" | "add";

export function CloudAssistantTab(): React.ReactElement {
  const { ws } = useParams({ strict: false }) as { ws?: string };
  const connections = useCloudConnections();
  const quota = useCloudQuota();
  const actions = useCloudAssistantActions();
  const [view, setView] = React.useState<View>("list");
  const [busyId, setBusyId] = React.useState<string | null>(null);
  const [testing, setTesting] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [addError, setAddError] = React.useState<string | null>(null);
  /** Per-ID results of the last refused create; the add form renders them inline. */
  const [addResults, setAddResults] = React.useState<ModelTestResults | null>(null);

  const signIn = useProviderSignIn();

  const list = connections.data;
  const flags = list === undefined ? null : connectionModelFlags(list, quota.data ?? null);
  const rows =
    list === undefined || flags === null ? [] : list.connections.map((row) => toConnectionRow(row, list, flags));
  const signInAvailable = list?.openai_login_available ?? false;
  const rawWalletUrl = list?.openhub_wallet_url ?? "";
  const topUpHref = rawWalletUrl.trim() === "" ? null : rawWalletUrl;

  const saveModels = async (row: ConnectionRow, models: string[], saveUnverified: boolean): Promise<void> => {
    setBusyId(row.id);
    try {
      await actions.setModels(row.id, models, saveUnverified);
      await actions.invalidate();
    } finally {
      setBusyId(null);
    }
  };

  const testModel = async (row: ConnectionRow, model: string): Promise<ModelTestResult> => {
    setBusyId(row.id);
    try {
      const result = await actions.testModel(row.id, model);
      await actions.invalidate();
      return result;
    } finally {
      setBusyId(null);
    }
  };

  const commitModel = async (row: ConnectionRow, model: string): Promise<void> => {
    // The meter gate lives on the hosted (OpenHub) lane: the codex lane
    // draws on the ChatGPT plan and self-supplied models draw on the
    // user's own key, so neither is credit-blocked here.
    if (row.kind === "openhub" && flags !== null && isBlockedCommit(model, flags)) {
      notifyError(MODEL_BLOCKED_TITLE, MODEL_BLOCKED_BODY);
      return;
    }
    setBusyId(row.id);
    try {
      await actions.setModel(row.id, model);
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

  const saveConnection = async (draft: ByokDraft, saveUnverified: boolean): Promise<void> => {
    setSaving(true);
    setAddError(null);
    try {
      const submission = byokSubmission(draft);
      await actions.create({
        providerKind: submission.providerKind,
        modelName: submission.modelName,
        models: submission.models,
        ...(saveUnverified ? { saveUnverified } : {}),
        apiKey: submission.apiKey,
        ...(submission.baseUrl === null ? {} : { baseUrl: submission.baseUrl }),
      });
      await actions.invalidate();
      notifySuccess(ADDED_TITLE);
      setAddResults(null);
      setView("list");
    } catch (error) {
      // A refused write (`400 model_test_failed`, nothing saved) stays in
      // the form per ID; any other failure notifies with the form error.
      const results = modelTestResultsFrom(error);
      if (results === null) {
        setAddError(actionFailureDetail(error, ADD_FAILED_FALLBACK));
        notifyError(ADD_FAILED_TITLE, actionFailureDetail(error, ADD_FAILED_FALLBACK));
      } else {
        setAddResults(results);
      }
    } finally {
      setSaving(false);
    }
  };

  const openAdd = (): void => {
    setAddError(null);
    setAddResults(null);
    setView("add");
  };

  return (
    <div className="flex max-w-2xl flex-col gap-(--space-6)">
      <header className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <h1 className="text-h2 text-(--text)">AI Assistant</h1>
        {view === "list" ? (
          <Button variant="primary" onClick={openAdd}>
            {ADD_ACTION}
          </Button>
        ) : (
          <Button variant="secondary" onClick={() => setView("list")}>
            {BACK_ACTION}
          </Button>
        )}
      </header>

      {view === "list" ? (
        <>
          {ws === undefined || list === undefined ? null : (
            <AssistantModelsSection workspaceSlug={ws} list={list} quota={quota.data ?? null} />
          )}
          <FreeTokenMeter />
        </>
      ) : null}

      {view === "add" ? (
        <AddConnectionView
          saving={saving}
          error={addError}
          results={addResults}
          onSave={(draft, saveUnverified) => void saveConnection(draft, saveUnverified)}
        />
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
          topUpHref={topUpHref}
          onCommitModel={(row, model) => void commitModel(row, model)}
          onSaveModels={(row, models, saveUnverified) => saveModels(row, models, saveUnverified)}
          onTestModel={(row, model) => testModel(row, model)}
          onTest={() => void testActive()}
          onRemove={(row) => void removeRow(row.id, row.title)}
          onDisconnect={(row) => void disconnectRow(row.id)}
          onSignIn={() => void signIn.signIn()}
        />
      )}

      {view === "list" ? (
        <>
          <hr className="border-(--border)" />
          <ToolServersSection />
          <AppsPointer />
        </>
      ) : null}
    </div>
  );
}

/** The row title helper is also used by tests to pin the naming rule. */
export { connectionTitle };
