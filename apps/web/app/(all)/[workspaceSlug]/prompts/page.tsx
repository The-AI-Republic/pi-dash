/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
import useSWR, { useSWRConfig } from "swr";
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
type TPromptPageTab = "sections" | "receipt";

type TKindSections = {
  kind: TPromptKind;
  sections: IResolvedSection[];
};

type TSectionEntry = {
  section: IResolvedSection;
  kinds: TPromptKind[];
};

type TReceiptEntry = {
  kind: TPromptKind;
  compiled: IPromptCompiledResponse;
  sections: IResolvedSection[];
};

function isPersonalSource(source: string): boolean {
  return source.startsWith("user:");
}

function sectionAnchorId(sectionKey: string): string {
  return `prompt-section-${sectionKey}`;
}

function receiptAnchorId(kind: TPromptKind): string {
  return `prompt-receipt-${kind}`;
}

function useKindLabel() {
  const { t } = useTranslation();

  // Static t("…") arms so the i18n extractor registers these message ids; a
  // runtime `t(variable)` would be invisible to it.
  return (k: TPromptKind): string => {
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
}

function usePromptSectionList(slug: string, kind: TPromptKind, scope: TPromptScope, enabled = true) {
  const promptStore = usePromptSection();
  const key = slug && enabled ? (["prompt-sections", slug, kind, scope] as const) : null;
  return useSWR<IPromptSectionListResponse>(key, () => promptStore.fetchSections(slug, kind, scope));
}

function useCompiledPrompt(slug: string, kind: TPromptKind) {
  const promptStore = usePromptSection();
  const key = slug ? (["prompt-compiled", slug, kind, "user"] as const) : null;
  return useSWR<IPromptCompiledResponse>(key, () => promptStore.fetchCompiled(slug, kind, "user"));
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

  const slug = workspaceSlug ?? "";
  const isAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE, slug);
  const [tab, setTab] = useState<TPromptPageTab>("sections");

  const pageTitle = currentWorkspace?.name ? `${currentWorkspace.name} · ${t("Prompts")}` : t("Prompts");

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHead title={pageTitle} />

      <header className="flex flex-col gap-1">
        <h1 className="text-16 font-semibold text-primary">{t("Prompts")}</h1>
        <p className="text-13 text-secondary">
          {t("Manage reusable sections and inspect the assembled prompt receipts.")}
        </p>
      </header>

      <div className="flex items-center gap-2">
        {(["sections", "receipt"] as TPromptPageTab[]).map((nextTab) => (
          <button
            key={nextTab}
            type="button"
            onClick={() => setTab(nextTab)}
            className={`rounded-md px-3 py-1.5 text-13 font-medium transition-colors ${
              tab === nextTab ? "bg-accent-primary text-on-color" : "bg-layer-1 text-secondary hover:text-primary"
            }`}
          >
            {nextTab === "sections" ? t("Sections") : t("Receipt")}
          </button>
        ))}
      </div>

      {tab === "sections" ? (
        <SectionsLibrary slug={slug} isAdmin={isAdmin} />
      ) : (
        <ReceiptLibrary slug={slug} isAdmin={isAdmin} />
      )}
    </div>
  );
});

function SectionsLibrary({ slug, isAdmin }: { slug: string; isAdmin: boolean }) {
  const { t } = useTranslation();
  const { mutate } = useSWRConfig();

  const codingUser = usePromptSectionList(slug, "coding-task", "user");
  const reviewUser = usePromptSectionList(slug, "review", "user");
  const schedulerUser = usePromptSectionList(slug, "scheduler", "user");
  const codingWs = usePromptSectionList(slug, "coding-task", "workspace", isAdmin);
  const reviewWs = usePromptSectionList(slug, "review", "workspace", isAdmin);
  const schedulerWs = usePromptSectionList(slug, "scheduler", "workspace", isAdmin);

  const userLists = useMemo<TKindSections[]>(
    () => [
      { kind: "coding-task", sections: codingUser.data?.sections ?? [] },
      { kind: "review", sections: reviewUser.data?.sections ?? [] },
      { kind: "scheduler", sections: schedulerUser.data?.sections ?? [] },
    ],
    [codingUser.data, reviewUser.data, schedulerUser.data]
  );
  const wsLists = useMemo<TKindSections[]>(
    () => [
      { kind: "coding-task", sections: codingWs.data?.sections ?? [] },
      { kind: "review", sections: reviewWs.data?.sections ?? [] },
      { kind: "scheduler", sections: schedulerWs.data?.sections ?? [] },
    ],
    [codingWs.data, reviewWs.data, schedulerWs.data]
  );

  const entries = useMemo<TSectionEntry[]>(() => {
    const map = new Map<string, TSectionEntry>();
    for (const { kind, sections } of userLists) {
      for (const section of sections) {
        const existing = map.get(section.key);
        if (existing) {
          if (!existing.kinds.includes(kind)) existing.kinds.push(kind);
        } else {
          map.set(section.key, { section, kinds: [kind] });
        }
      }
    }
    return Array.from(map.values());
  }, [userLists]);

  const wsByKey = useMemo(() => {
    const map: Record<string, IResolvedSection> = {};
    for (const { sections } of wsLists) {
      for (const section of sections) map[section.key] = section;
    }
    return map;
  }, [wsLists]);

  const userError = codingUser.error || reviewUser.error || schedulerUser.error;
  const wsError = codingWs.error || reviewWs.error || schedulerWs.error;
  const userReady = codingUser.data !== undefined && reviewUser.data !== undefined && schedulerUser.data !== undefined;
  const workspaceReady =
    !isAdmin || (codingWs.data !== undefined && reviewWs.data !== undefined && schedulerWs.data !== undefined);

  async function refresh() {
    await Promise.all([
      codingUser.mutate(),
      reviewUser.mutate(),
      schedulerUser.mutate(),
      codingWs.mutate(),
      reviewWs.mutate(),
      schedulerWs.mutate(),
      ...KINDS.map((kind) => mutate(["prompt-compiled", slug, kind, "user"] as const)),
    ]);
  }

  if (userError) {
    return (
      <div className="rounded-md border border-danger-subtle bg-layer-1 p-4 text-13 text-danger-primary">
        {t("Could not load prompt sections for this workspace.")}
      </div>
    );
  }

  if (!userReady) return <div className="text-13 text-secondary">{t("Loading…")}</div>;

  return (
    <section className="grid gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
      <SectionNavigation entries={entries} />
      <div className="flex min-w-0 flex-col gap-3">
        {isAdmin && wsError && (
          // The workspace-scope list is what gates the "Customize for
          // workspace" buttons; if it failed, say so rather than leave an
          // admin staring at silently-missing controls.
          <div className="rounded-md border border-warning-subtle bg-layer-1 p-3 text-11 text-warning-primary">
            {t(
              "Couldn't load this workspace's section defaults, so workspace editing is unavailable. Reload to try again."
            )}
          </div>
        )}
        {entries.map(({ section, kinds }) => (
          <SectionCard
            key={section.key}
            sectionId={sectionAnchorId(section.key)}
            slug={slug}
            previewKinds={kinds}
            section={section}
            workspaceSection={wsByKey[section.key]}
            workspaceReady={workspaceReady}
            isAdmin={isAdmin}
            onChanged={refresh}
          />
        ))}
      </div>
    </section>
  );
}

function SectionNavigation({ entries }: { entries: TSectionEntry[] }) {
  const { t } = useTranslation();

  return (
    <aside className="self-start rounded-md border border-subtle bg-layer-1 lg:sticky lg:top-4">
      <div className="border-b border-subtle px-3 py-2 text-11 font-medium text-secondary">{t("Sections")}</div>
      <nav className="flex max-h-[calc(100vh-12rem)] flex-col gap-1 overflow-auto p-2">
        {entries.map(({ section }) => (
          <a
            key={section.key}
            href={`#${sectionAnchorId(section.key)}`}
            className="rounded px-2 py-1.5 text-12 text-secondary hover:bg-layer-2 hover:text-primary"
          >
            <span className="block truncate">{section.title}</span>
            <span className="block truncate text-10 text-placeholder">{section.key}</span>
          </a>
        ))}
      </nav>
    </aside>
  );
}

function ReceiptLibrary({ slug, isAdmin }: { slug: string; isAdmin: boolean }) {
  const { t } = useTranslation();
  const coding = useCompiledPrompt(slug, "coding-task");
  const review = useCompiledPrompt(slug, "review");
  const scheduler = useCompiledPrompt(slug, "scheduler");
  const codingSections = usePromptSectionList(slug, "coding-task", "user");
  const reviewSections = usePromptSectionList(slug, "review", "user");
  const schedulerSections = usePromptSectionList(slug, "scheduler", "user");

  const compiledByKind: Partial<Record<TPromptKind, IPromptCompiledResponse>> = {
    "coding-task": coding.data,
    review: review.data,
    scheduler: scheduler.data,
  };
  const sectionsByKind: Partial<Record<TPromptKind, IResolvedSection[]>> = {
    "coding-task": codingSections.data?.sections,
    review: reviewSections.data?.sections,
    scheduler: schedulerSections.data?.sections,
  };
  const error =
    coding.error ||
    review.error ||
    scheduler.error ||
    codingSections.error ||
    reviewSections.error ||
    schedulerSections.error;
  const ready =
    coding.data !== undefined &&
    review.data !== undefined &&
    scheduler.data !== undefined &&
    codingSections.data !== undefined &&
    reviewSections.data !== undefined &&
    schedulerSections.data !== undefined;

  const entries: TReceiptEntry[] = [];
  for (const kind of KINDS) {
    const compiled = compiledByKind[kind];
    const sections = sectionsByKind[kind];
    if (compiled && sections) entries.push({ kind, compiled, sections });
  }

  if (error) {
    return (
      <div className="rounded-md border border-danger-subtle bg-layer-1 p-4 text-13 text-danger-primary">
        {t("Could not load prompt receipts for this workspace.")}
      </div>
    );
  }

  if (!ready) return <div className="text-13 text-secondary">{t("Loading…")}</div>;

  return (
    <section className="grid gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
      <ReceiptNavigation entries={entries} />
      <div className="flex min-w-0 flex-col gap-3">
        <div className="rounded-md border border-subtle bg-layer-1 px-4 py-3">
          <h2 className="text-13 font-medium text-primary">{t("Receipt")}</h2>
          <p className="mt-1 text-12 text-secondary">
            {t(
              "A receipt is the final prompt template assembled from ordered sections. Editing a section changes the matching part of every receipt that includes it."
            )}
          </p>
        </div>
        {entries.map(({ kind, compiled, sections }) => (
          <ReceiptCard
            key={kind}
            receiptId={receiptAnchorId(kind)}
            slug={slug}
            kind={kind}
            compiled={compiled}
            sections={sections}
            isAdmin={isAdmin}
          />
        ))}
      </div>
    </section>
  );
}

function ReceiptNavigation({ entries }: { entries: TReceiptEntry[] }) {
  const { t } = useTranslation();
  const kindLabel = useKindLabel();

  return (
    <aside className="self-start rounded-md border border-subtle bg-layer-1 lg:sticky lg:top-4">
      <div className="border-b border-subtle px-3 py-2 text-11 font-medium text-secondary">{t("Receipts")}</div>
      <nav className="flex max-h-[calc(100vh-12rem)] flex-col gap-1 overflow-auto p-2">
        {entries.map(({ kind, sections }) => (
          <a
            key={kind}
            href={`#${receiptAnchorId(kind)}`}
            className="rounded px-2 py-1.5 text-12 text-secondary hover:bg-layer-2 hover:text-primary"
          >
            <span className="block truncate">{kindLabel(kind)}</span>
            <span className="block truncate text-10 text-placeholder">
              {t("{{count}} sections", { count: sections.length })}
            </span>
          </a>
        ))}
      </nav>
    </aside>
  );
}

// ----------------------------------------------------------------------
// Section card + inline editor
// ----------------------------------------------------------------------

type SectionCardProps = {
  sectionId: string;
  slug: string;
  previewKinds: TPromptKind[];
  /** Effective (user-scope) resolution of the section. */
  section: IResolvedSection;
  /** Workspace-scope resolution of the same section, if loaded. */
  workspaceSection: IResolvedSection | undefined;
  /** Whether the workspace-scope list has resolved (so an override, if any, is known). */
  workspaceReady: boolean;
  isAdmin: boolean;
  onChanged: () => Promise<void>;
};

function SectionCard({
  sectionId,
  slug,
  previewKinds,
  section,
  workspaceSection,
  workspaceReady,
  isAdmin,
  onChanged,
}: SectionCardProps) {
  const { t } = useTranslation();
  const kindLabel = useKindLabel();
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
    <div id={sectionId} className="scroll-mt-4 rounded-md border border-subtle">
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
          <div className="flex flex-wrap gap-1.5">
            {previewKinds.map((kind) => (
              <Badge key={kind} variant="accent-neutral" size="sm">
                {kindLabel(kind)}
              </Badge>
            ))}
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
          previewKinds={previewKinds}
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
  previewKinds: TPromptKind[];
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
  previewKinds,
  sectionKey,
  scope,
  seed,
  defaultBody,
  hasOverride,
  onClose,
  onChanged,
}: SectionEditorProps) {
  const { t } = useTranslation();
  const kindLabel = useKindLabel();
  const promptStore = usePromptSection();
  const [draft, setDraft] = useState(seed);
  const [previewKind, setPreviewKind] = useState<TPromptKind>(previewKinds[0] ?? "coding-task");
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

      <div className="flex flex-col gap-2 rounded-md border border-subtle bg-layer-1 p-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-11 font-medium text-secondary">{t("Preview draft")}</span>
          {previewKinds.length > 1 && (
            <div className="flex items-center gap-1">
              {previewKinds.map((kind) => (
                <button
                  key={kind}
                  type="button"
                  onClick={() => setPreviewKind(kind)}
                  className={`rounded px-2 py-1 text-11 transition-colors ${
                    previewKind === kind
                      ? "bg-accent-primary text-on-color"
                      : "bg-layer-2 text-secondary hover:text-primary"
                  }`}
                >
                  {kindLabel(kind)}
                </button>
              ))}
            </div>
          )}
        </div>
        <PromptPreviewForm
          slug={slug}
          kind={previewKind}
          submitLabel={t("Preview draft")}
          draft={{ scope, sectionKey, body: draft }}
          nested
        />
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

function ReceiptCard({
  receiptId,
  slug,
  kind,
  compiled,
  sections,
  isAdmin,
}: {
  receiptId: string;
  slug: string;
  kind: TPromptKind;
  compiled: IPromptCompiledResponse;
  sections: IResolvedSection[];
  isAdmin: boolean;
}) {
  const { t } = useTranslation();
  const kindLabel = useKindLabel();
  const [open, setOpen] = useState(false);

  return (
    <section id={receiptId} className="scroll-mt-4 rounded-md border border-subtle">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-13 font-medium text-primary"
      >
        <span className="flex items-center gap-2">
          {kindLabel(kind)}
          <Badge variant="accent-neutral" size="sm">
            {t("{{count}} sections", { count: sections.length })}
          </Badge>
        </span>
        <span className="text-11 text-secondary">{open ? t("Hide") : t("Show")}</span>
      </button>
      <div className="border-t border-subtle px-4 py-3">
        <h3 className="text-11 font-medium text-secondary">{t("Composed from")}</h3>
        <ol className="mt-2 grid gap-1.5 md:grid-cols-2">
          {sections.map((section, index) => (
            <li key={section.key} className="flex min-w-0 items-center gap-2 rounded bg-layer-1 px-2 py-1.5">
              <span className="font-mono text-10 text-placeholder">{String(index + 1).padStart(2, "0")}</span>
              <div className="min-w-0 flex-1">
                <span className="block truncate text-12 text-primary">{section.title}</span>
                <span className="block truncate text-10 text-placeholder">{section.key}</span>
              </div>
              <SourceBadge source={section.source} />
            </li>
          ))}
        </ol>
      </div>
      {open && (
        <div className="flex flex-col gap-3 border-t border-subtle px-4 py-3">
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
          {isAdmin && (
            <div className="flex flex-col gap-2 border-t border-subtle pt-3">
              <h2 className="text-13 font-medium text-primary">{t("Preview")}</h2>
              <PromptPreviewForm slug={slug} kind={kind} submitLabel={t("Preview")} />
            </div>
          )}
        </div>
      )}
    </section>
  );
}

type PromptPreviewFormProps = {
  slug: string;
  kind: TPromptKind;
  submitLabel: string;
  /** When set, previews an unsaved draft of this section instead of the saved prompt. */
  draft?: { scope: TPromptScope; sectionKey: string; body: string };
  /** Use layer-2 surfaces so the form reads correctly when embedded in a layer-1 card. */
  nested?: boolean;
};

/**
 * Shared "render against a real issue/binding" form used both standalone (the
 * admin PreviewPanel) and inside the section editor (unsaved-draft preview).
 */
function PromptPreviewForm({ slug, kind, submitLabel, draft, nested }: PromptPreviewFormProps) {
  const { t } = useTranslation();
  const promptStore = usePromptSection();
  const [target, setTarget] = useState("");
  const [prompt, setPrompt] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isScheduler = kind === "scheduler";
  const surface = nested ? "bg-layer-2" : "bg-layer-1";

  async function run() {
    if (loading) return;
    if (!target) {
      setError(isScheduler ? t("Enter a scheduler binding id first.") : t("Enter an issue id first."));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const targetField = isScheduler ? { binding_id: target } : { issue_id: target };
      const payload = draft
        ? { ...targetField, scope: draft.scope, section_key: draft.sectionKey, body: draft.body }
        : targetField;
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
    <>
      <div className="flex items-center gap-2">
        <input
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          placeholder={isScheduler ? t("Scheduler binding id (UUID)") : t("Issue id (UUID)")}
          className={`font-mono flex-1 rounded-md border border-subtle ${surface} px-3 py-1.5 text-11 text-primary focus:border-accent-strong focus:outline-none`}
        />
        <Button size="sm" variant="outline-primary" onClick={run} loading={loading} disabled={loading || !target}>
          {submitLabel}
        </Button>
      </div>
      {error && (
        <div className={`rounded border border-danger-subtle ${surface} p-2 text-11 text-danger-primary`}>{error}</div>
      )}
      {prompt && (
        <pre
          className={`font-mono max-h-[60vh] overflow-auto rounded-md border border-subtle ${surface} p-3 text-11 leading-5 whitespace-pre-wrap text-primary`}
        >
          {prompt}
        </pre>
      )}
    </>
  );
}

export default PromptsListPage;
