/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
import useSWR from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import type {
  IPromptCompiledResponse,
  IPromptSectionListResponse,
  IResolvedSection,
  TPromptKind,
  TPromptScope,
} from "@pi-dash/types";
import { AlertModalCore, Badge, Button } from "@pi-dash/ui";
import { PageHead } from "@/components/core/page-title";
import { usePromptSection } from "@/hooks/store/use-prompt-section";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";

const KINDS: TPromptKind[] = ["coding-task", "review", "scheduler"];

function isPersonalSource(source: string): boolean {
  return source.startsWith("user:");
}

/**
 * Section-based prompt customization. The prompt an agent runs is assembled
 * from ordered, code-owned *sections*; this page lets a workspace admin edit
 * the workspace default of the overridable sections and any member keep a
 * personal override, shows locked sections read-only, and renders the final
 * assembled "receipt" plus a live preview against a real issue.
 */
const PromptsListPage = observer(function PromptsListPage() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { currentWorkspace } = useWorkspace();
  const { allowPermissions } = useUserPermissions();
  const { t } = useTranslation();
  const promptStore = usePromptSection();

  const slug = workspaceSlug ?? "";
  const isAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, slug);

  // Static t("…") arms so the i18n extractor registers these message ids; a
  // runtime `t(variable)` would be invisible to it.
  const kindLabel = (k: TPromptKind): string => {
    switch (k) {
      case "coding-task":
        return t("Coding task");
      case "review":
        return t("Review");
      case "scheduler":
        return t("Scheduler");
      default:
        return k;
    }
  };

  const [kind, setKind] = useState<TPromptKind>("coding-task");

  // Effective view for the current user (personal → workspace → default), the
  // workspace-only baseline (so an admin edits the shared default and members
  // see it), and the assembled receipt.
  const userKey = slug ? (["prompt-sections", slug, kind, "user"] as const) : null;
  // Only admins can read/write at workspace scope, so members never fetch it.
  const wsKey = slug && isAdmin ? (["prompt-sections", slug, kind, "workspace"] as const) : null;
  const compiledKey = slug ? (["prompt-compiled", slug, kind, "user"] as const) : null;

  const {
    data: userData,
    error: userError,
    mutate: mutateUser,
  } = useSWR<IPromptSectionListResponse>(userKey, () => promptStore.fetchSections(slug, kind, "user"));
  const { data: wsData, mutate: mutateWs } = useSWR<IPromptSectionListResponse>(wsKey, () =>
    promptStore.fetchSections(slug, kind, "workspace")
  );
  const { data: compiled, mutate: mutateCompiled } = useSWR<IPromptCompiledResponse>(compiledKey, () =>
    promptStore.fetchCompiled(slug, kind, "user")
  );

  const wsByKey = useMemo(() => {
    const map: Record<string, IResolvedSection> = {};
    for (const s of wsData?.sections ?? []) map[s.key] = s;
    return map;
  }, [wsData]);

  async function refresh() {
    await Promise.all([mutateUser(), mutateWs(), mutateCompiled()]);
  }

  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} · ${t("Prompts")}` : t("Prompts");
  const sections = userData?.sections ?? [];

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHead title={pageTitle} />

      <header className="flex flex-col gap-1">
        <h1 className="text-16 font-semibold text-primary">{t("Prompts")}</h1>
        <p className="text-13 text-secondary">
          {t(
            "The prompt an agent runs is assembled from ordered sections. Admins can customize the overridable sections for the whole workspace; you can keep your own personal overrides for runs you trigger. Locked sections are fixed."
          )}
        </p>
      </header>

      <div className="flex items-center gap-2">
        {KINDS.map((k) => (
          <button
            key={k}
            type="button"
            onClick={() => setKind(k)}
            className={`rounded-md px-3 py-1.5 text-13 font-medium transition-colors ${
              kind === k ? "bg-accent-primary text-on-color" : "bg-layer-1 text-secondary hover:text-primary"
            }`}
          >
            {kindLabel(k)}
          </button>
        ))}
      </div>

      {userError ? (
        <div className="rounded-md border border-danger-subtle bg-layer-1 p-4 text-13 text-danger-primary">
          {t("Could not load prompt sections for this workspace.")}
        </div>
      ) : userData === undefined ? (
        <div className="text-13 text-secondary">{t("Loading…")}</div>
      ) : (
        <section className="flex flex-col gap-3">
          {sections.map((section) => (
            // Key by kind too: a section shared across recipes (session-framing,
            // guardrails, …) must remount on tab switch, not carry its open
            // editor and unsaved draft into the other kind's context.
            <SectionCard
              key={`${kind}:${section.key}`}
              slug={slug}
              section={section}
              workspaceSection={wsByKey[section.key]}
              workspaceReady={wsData !== undefined}
              isAdmin={isAdmin}
              onChanged={refresh}
            />
          ))}
        </section>
      )}

      {compiled && <AssembledPanel compiled={compiled} />}

      {isAdmin && <PreviewPanel slug={slug} kind={kind} />}
    </div>
  );
});

// ----------------------------------------------------------------------
// Section card + inline editor
// ----------------------------------------------------------------------

type SectionCardProps = {
  slug: string;
  /** Effective (user-scope) resolution of the section. */
  section: IResolvedSection;
  /** Workspace-scope resolution of the same section, if loaded. */
  workspaceSection: IResolvedSection | undefined;
  /** Whether the workspace-scope list has resolved (so an override, if any, is known). */
  workspaceReady: boolean;
  isAdmin: boolean;
  onChanged: () => Promise<void>;
};

function SectionCard({ slug, section, workspaceSection, workspaceReady, isAdmin, onChanged }: SectionCardProps) {
  const { t } = useTranslation();
  const [editScope, setEditScope] = useState<TPromptScope | null>(null);

  // Gate workspace editing on the workspace-scope fetch having resolved: until
  // then we can't tell "no override" from "not loaded yet", and seeding the
  // editor from the registry default would let Save silently overwrite an
  // existing workspace override.
  const canEditWorkspace = section.editable_at_workspace && isAdmin && workspaceReady;
  const canEditPersonal = section.editable_at_personal;

  const hasWorkspaceOverride = workspaceSection?.source === "workspace";
  const hasPersonalOverride = isPersonalSource(section.source);

  // Seed the editor from the override that already exists at the chosen scope,
  // falling back to the workspace baseline / registry default.
  const seedFor = (scope: TPromptScope): string => {
    if (scope === "workspace") return workspaceSection?.body ?? section.default_body;
    return section.body; // effective body is the best personal starting point
  };

  return (
    <div className="rounded-md border border-subtle">
      <div className="flex items-start justify-between gap-3 px-4 py-3">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <span className="text-13 font-medium text-primary">{section.title}</span>
            <span className="text-11 text-placeholder">{section.key}</span>
            <SourceBadge source={section.source} />
            {section.customizable === "locked" && (
              <Badge variant="accent-neutral" size="sm">
                {t("Locked")}
              </Badge>
            )}
            {section.customizable === "workspace" && (
              <Badge variant="accent-neutral" size="sm">
                {t("Admin-managed")}
              </Badge>
            )}
          </div>
          {section.needs_attention && (
            <span className="text-11 text-warning-primary">
              {t("This override may no longer render after a recent change — review and re-save it.")}
            </span>
          )}
        </div>
        {editScope === null && (
          <div className="flex shrink-0 items-center gap-2">
            {canEditWorkspace && (
              <Button size="sm" variant="outline-primary" onClick={() => setEditScope("workspace")}>
                {hasWorkspaceOverride ? t("Edit workspace default") : t("Customize for workspace")}
              </Button>
            )}
            {canEditPersonal && (
              <Button size="sm" variant="outline-primary" onClick={() => setEditScope("user")}>
                {hasPersonalOverride ? t("Edit my override") : t("Customize for me")}
              </Button>
            )}
          </div>
        )}
      </div>

      {editScope === null ? (
        <pre className="font-mono max-h-64 overflow-auto border-t border-subtle bg-layer-1 px-4 py-3 text-11 leading-5 whitespace-pre-wrap text-primary">
          {section.body}
        </pre>
      ) : (
        <SectionEditor
          slug={slug}
          sectionKey={section.key}
          scope={editScope}
          seed={seedFor(editScope)}
          defaultBody={section.default_body}
          hasOverride={editScope === "workspace" ? hasWorkspaceOverride : hasPersonalOverride}
          onClose={() => setEditScope(null)}
          onChanged={onChanged}
        />
      )}
    </div>
  );
}

function SourceBadge({ source }: { source: string }) {
  const { t } = useTranslation();
  if (source === "workspace")
    return (
      <Badge variant="accent-primary" size="sm">
        {t("Workspace override")}
      </Badge>
    );
  if (isPersonalSource(source))
    return (
      <Badge variant="accent-primary" size="sm">
        {t("Your override")}
      </Badge>
    );
  return (
    <Badge variant="accent-neutral" size="sm">
      {t("Pi Dash default")}
    </Badge>
  );
}

type SectionEditorProps = {
  slug: string;
  sectionKey: string;
  scope: TPromptScope;
  seed: string;
  defaultBody: string;
  hasOverride: boolean;
  onClose: () => void;
  onChanged: () => Promise<void>;
};

function SectionEditor({
  slug,
  sectionKey,
  scope,
  seed,
  defaultBody,
  hasOverride,
  onClose,
  onChanged,
}: SectionEditorProps) {
  const { t } = useTranslation();
  const promptStore = usePromptSection();
  const [draft, setDraft] = useState(seed);
  const [saving, setSaving] = useState(false);
  const [reverting, setReverting] = useState(false);
  const [showRevertConfirm, setShowRevertConfirm] = useState(false);
  const [showDefault, setShowDefault] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dirty = draft !== seed;

  async function handleSave() {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      await promptStore.upsertSection(slug, sectionKey, { scope, body: draft });
      await onChanged();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Section saved"),
        message:
          scope === "workspace"
            ? t("Subsequent agent runs in this workspace will use the updated section.")
            : t("Runs you trigger will use your updated section."),
      });
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string; detail?: string } | null;
      setError(err?.detail ?? err?.error ?? t("Could not save the section."));
    } finally {
      setSaving(false);
    }
  }

  async function handleRevert() {
    if (reverting) return;
    setReverting(true);
    setError(null);
    try {
      await promptStore.revertSection(slug, sectionKey, scope);
      await onChanged();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: t("Reverted to default"),
        message: t("This section is back on the shared default."),
      });
      onClose();
    } catch (e: unknown) {
      const err = e as { error?: string } | null;
      setError(err?.error ?? t("Could not revert the section."));
      // Close the dialog so the inline error is visible behind it.
      setShowRevertConfirm(false);
    } finally {
      setReverting(false);
    }
  }

  return (
    <div className="flex flex-col gap-2 border-t border-subtle px-4 py-3">
      <div className="flex items-center justify-between">
        <span className="text-11 font-medium text-secondary">
          {scope === "workspace"
            ? t("Workspace default (Jinja + Markdown)")
            : t("Your personal override (Jinja + Markdown)")}
        </span>
        <button
          type="button"
          onClick={() => setShowDefault((v) => !v)}
          className="text-11 text-secondary hover:text-primary"
        >
          {showDefault ? t("Hide default") : t("Compare with default")}
        </button>
      </div>

      <div className={`grid gap-2 ${showDefault ? "lg:grid-cols-2" : ""}`}>
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          spellCheck={false}
          className="font-mono min-h-[40vh] w-full resize-y rounded-md border border-subtle bg-layer-1 p-3 text-11 leading-5 text-primary focus:border-accent-strong focus:outline-none"
        />
        {showDefault && (
          <pre className="font-mono min-h-[40vh] w-full overflow-auto rounded-md border border-subtle bg-layer-2 p-3 text-11 leading-5 whitespace-pre-wrap text-secondary">
            {defaultBody}
          </pre>
        )}
      </div>

      {error && (
        <div className="rounded border border-danger-subtle bg-layer-1 p-2 text-11 text-danger-primary">{error}</div>
      )}

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={handleSave} loading={saving} disabled={!dirty || saving}>
            {t("Save")}
          </Button>
          <Button size="sm" variant="neutral-primary" onClick={onClose} disabled={saving || reverting}>
            {t("Cancel")}
          </Button>
        </div>
        {hasOverride && (
          <Button size="sm" variant="tertiary-danger" onClick={() => setShowRevertConfirm(true)} disabled={reverting}>
            {t("Revert to default")}
          </Button>
        )}
      </div>

      <AlertModalCore
        isOpen={showRevertConfirm}
        handleClose={() => (reverting ? null : setShowRevertConfirm(false))}
        handleSubmit={handleRevert}
        isSubmitting={reverting}
        title={t("Revert to default?")}
        content={
          scope === "workspace"
            ? t(
                "New agent runs in this workspace will use the Pi Dash default for this section, for every member. This can't be undone."
              )
            : t("Runs you trigger will use the shared default for this section again. This can't be undone.")
        }
        primaryButtonText={{ default: t("Revert"), loading: t("Reverting") }}
      />
    </div>
  );
}

// ----------------------------------------------------------------------
// Assembled "receipt" + preview
// ----------------------------------------------------------------------

function AssembledPanel({ compiled }: { compiled: IPromptCompiledResponse }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <section className="rounded-md border border-subtle">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-13 font-medium text-primary"
      >
        <span>{t("Assembled prompt")}</span>
        <span className="text-11 text-secondary">{open ? t("Hide") : t("Show")}</span>
      </button>
      {open && (
        <div className="flex flex-col gap-3 border-t border-subtle px-4 py-3">
          <p className="text-11 text-secondary">
            {t(
              "This is how the sections above combine into the final template (Jinja markers intact) for runs you trigger."
            )}
          </p>
          <pre className="font-mono max-h-[60vh] overflow-auto rounded-md border border-subtle bg-layer-1 p-3 text-11 leading-5 whitespace-pre-wrap text-primary">
            {compiled.template_body}
          </pre>
          {compiled.automatic_template_body && (
            <>
              <p className="text-11 text-secondary">
                {t("Automatic runs (scheduler, ticks) ignore your personal overrides and get this instead:")}
              </p>
              <pre className="font-mono max-h-[40vh] overflow-auto rounded-md border border-subtle bg-layer-2 p-3 text-11 leading-5 whitespace-pre-wrap text-secondary">
                {compiled.automatic_template_body}
              </pre>
            </>
          )}
        </div>
      )}
    </section>
  );
}

function PreviewPanel({ slug, kind }: { slug: string; kind: TPromptKind }) {
  const { t } = useTranslation();
  const promptStore = usePromptSection();
  const [target, setTarget] = useState("");
  const [prompt, setPrompt] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isScheduler = kind === "scheduler";

  async function handlePreview() {
    if (loading) return;
    if (!target) {
      setError(isScheduler ? t("Enter a scheduler binding id first.") : t("Enter an issue id first."));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const payload = isScheduler ? { binding_id: target } : { issue_id: target };
      const resp = await promptStore.previewPrompt(slug, kind, payload);
      setPrompt(resp.prompt);
    } catch (e: unknown) {
      const err = e as { error?: string; detail?: string } | null;
      setError(err?.detail ?? err?.error ?? t("Render failed."));
      setPrompt("");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="flex flex-col gap-2 rounded-md border border-subtle px-4 py-3">
      <h2 className="text-13 font-medium text-primary">{t("Preview")}</h2>
      <p className="text-11 text-secondary">
        {t("Render the assembled prompt against real data, without starting a run.")}
      </p>
      <div className="flex items-center gap-2">
        <input
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          placeholder={isScheduler ? t("Scheduler binding id (UUID)") : t("Issue id (UUID)")}
          className="font-mono flex-1 rounded-md border border-subtle bg-layer-1 px-3 py-1.5 text-11 text-primary focus:border-accent-strong focus:outline-none"
        />
        <Button
          size="sm"
          variant="outline-primary"
          onClick={handlePreview}
          loading={loading}
          disabled={loading || !target}
        >
          {t("Preview")}
        </Button>
      </div>
      {error && (
        <div className="rounded border border-danger-subtle bg-layer-1 p-2 text-11 text-danger-primary">{error}</div>
      )}
      {prompt && (
        <pre className="font-mono max-h-[60vh] overflow-auto rounded-md border border-subtle bg-layer-1 p-3 text-11 leading-5 whitespace-pre-wrap text-primary">
          {prompt}
        </pre>
      )}
    </section>
  );
}

export default PromptsListPage;
