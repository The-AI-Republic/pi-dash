// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editable page canvas (parity/specs/pages.md §5, PAGE-026–032). One
// component owns the page's editing pieces and the seams between them:
// the canvas header holds the mark and the title, the title's Enter key
// hands the caret to the body's start, the sticky bar drives the
// editor's commands, and the editor reports its formatting state back so
// the bar reflects the block under the caret. The read-only body is the
// Suspense fallback, so a slow editor chunk never leaves the canvas
// blank.

import * as React from "react";

import { pageAssetUrl } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import {
  EMPTY_FORMATTING_STATE,
  type PageBlockKind,
  type PageFormatCommand,
  type PageFormattingState,
} from "../../../shared/editor/index.js";
import { descriptionAssetSrc } from "../../../shared/editor/index.js";
import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { getPageBodyDraft, setPageBodyDraft } from "../lib/pageBodyDraft.js";
import { usePageOversizedIncident } from "../lib/pageRealtimeErrors.js";
import type { PageFontSize, PageFontStyle } from "../lib/pagePreferences.js";
import { pageImageUploadFailure, uploadPageImage, type UploadedPageImage } from "../lib/pageMedia.js";
import { isPageSaveGesture, pageKeyCommand } from "../lib/pageToolbar.js";
import { clonePastedAssets, rewritePastedAssets } from "../lib/pageMedia.js";
import { parseEntityAddress } from "../lib/entityReferences.js";
import { entityReferenceHtml, pageEntityReferenceFor, resolvePastedEntity } from "../lib/pageEmbeds.js";
import { pageBodyActionsFor, registerPageBodyActions } from "../lib/pageBodyActions.js";
import { titleIsEmpty } from "../lib/pageTitle.js";
import type { PageLiveDocBinding } from "../lib/pageLiveBackend.js";
import { PageBodyEditor, type PageEditorHandle } from "./PageBodyEditor.js";
import { PageCanvasHeader } from "./PageCanvasHeader.js";
import { PageColorButton } from "./PageColorButton.js";
import { PageBodySkeleton } from "./PageDetailLoading.js";
import { PageFormattingBar } from "./PageFormattingBar.js";
import { PageOversizedStrip } from "./PageOversizedStrip.js";
import { PageImageButton } from "./PageImageButton.js";
import { PageTableButton } from "./PageTableButton.js";
import { PageBodyInteractionLayer } from "./PageBodyInteractionLayer.js";

export const PAGE_BODY_LABEL = "Page body";
export const PAGE_BODY_PLACEHOLDER = "Start writing, or press / for commands";
/** Id of the shared file input the catalogue's Image entry opens. */
export const PAGE_IMAGE_INPUT_ID = "page-image-input";

export interface PageEditorSurfaceProps {
  pageId: string;
  /** Workspace the page belongs to: asset URLs and upload grants (PAGE-034). */
  workspaceSlug: string;
  /** Project the page belongs to. */
  projectId: string;
  /** Stored record name. */
  name: string;
  /** Stored body HTML; a draft typed this session wins over it. */
  descriptionHtml: string;
  /** Stored `logo_props`, drawn in the canvas header. */
  logoProps: Record<string, unknown>;
  /** Persist the record name (title saves outside the body path). */
  onRename: (name: string) => void;
  /** Persist the chosen mark (an empty record clears it). */
  onMarkPick: (draft: Record<string, unknown>) => void;
  /** Whether the sticky bar renders (read-only pages never mount here). */
  showBar: boolean;
  /**
   * Receive the standard save gesture. The save slice (PAGE-064) forces
   * its manual fallback save here; the chord itself is always captured so
   * the browser dialog never opens (PAGE-073).
   */
  onSave?: (() => void) | undefined;
  /** The stored full-width preference: the bar content marks itself wide. */
  fullWidth?: boolean | undefined;
  /** The stored type size and typeface (PAGE-072); no switcher exists. */
  fontSize?: PageFontSize | undefined;
  fontStyle?: PageFontStyle | undefined;
  /**
   * The cold-document skeleton holds the body area until the page's
   * document is known (PAGE-079); the header and bar render throughout.
   */
  bodyPending?: boolean | undefined;
  /**
   * The live editor handle, so a caller outside the canvas can write the
   * body (the version overlay's restore, PAGE-053). Reported as null
   * before the editor chunk resolves.
   */
  onHandle?: ((handle: PageEditorHandle | null) => void) | undefined;
  /**
   * Upload one picked picture. The default goes through the shared page
   * asset contract; tests inject a stub instead of running the pipeline.
   */
  uploadImage?: ((file: File) => Promise<UploadedPageImage>) | undefined;
  /**
   * The live document the editors bind (PAGE-065). Absent the session
   * has not produced one and both editors stand alone on stored content.
   */
  collab?: PageLiveDocBinding | null | undefined;
  /** Seed empty shared fields from stored content once the server state is known. */
  seedDocument?: boolean | undefined;
  /**
   * The assistant handle may mount in the interaction layer: the cloud
   * flag, edit permission and a configured model all hold (PAGE-075).
   */
  assistantReady?: boolean | undefined;
}

/**
 * Open the toolbar's file input from another entry (the catalogue's
 * Image row). One picker exists per surface, so this never forks one.
 */
export function openPageImagePicker(): void {
  document.getElementById(PAGE_IMAGE_INPUT_ID)?.click();
}

export function PageEditorSurface({
  pageId,
  workspaceSlug,
  projectId,
  name,
  descriptionHtml,
  logoProps,
  onRename,
  onMarkPick,
  showBar,
  onSave,
  fullWidth = false,
  fontSize,
  fontStyle,
  bodyPending = false,
  onHandle,
  uploadImage,
  collab,
  seedDocument,
  assistantReady = false,
}: PageEditorSurfaceProps): React.ReactElement {
  const handleRef = React.useRef<PageEditorHandle | null>(null);
  const [state, setState] = React.useState<PageFormattingState>(EMPTY_FORMATTING_STATE);
  const [uploading, setUploading] = React.useState(false);
  // The oversized strip sits under the toolbar while a too-large incident
  // is live for this page (PAGE-063); only editors mount this surface.
  const oversized = usePageOversizedIncident(pageId);
  const initialHtml = React.useMemo(() => getPageBodyDraft(pageId) ?? descriptionHtml, [pageId, descriptionHtml]);
  // Latest body HTML, so the interaction layer can re-read the caret's
  // trigger after every edit without owning the editor itself.
  const [bodyHtml, setBodyHtml] = React.useState(initialHtml);
  // The typed title, before its debounce saves it: an empty draft keeps
  // the mark affordance pinned even though the stored name still has text.
  const [titleDraft, setTitleDraft] = React.useState(name);
  React.useEffect(() => setTitleDraft(name), [name]);
  // A stored body keeps the asset id; the editor paints the served URL
  // while serialization writes the id back (PAGE-034, ISS-127).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId }),
    [workspaceSlug, projectId]
  );

  const onHandleRef = React.useRef(onHandle);
  onHandleRef.current = onHandle;
  const saveRef = React.useRef(onSave);
  saveRef.current = onSave;

  const onReady = React.useCallback((handle: PageEditorHandle) => {
    handleRef.current = handle;
    setState(handle.readState());
    onHandleRef.current?.(handle);
  }, []);

  // A caller that outlives the editor (a restore open while the editor
  // unmounts) must not keep a handle to a dead document.
  React.useEffect(
    () => () => {
      handleRef.current = null;
      onHandleRef.current?.(null);
    },
    []
  );

  const onChange = React.useCallback(
    (html: string) => {
      // The save slice (PAGE-064, NEWFRONT-846) subscribes to this seam.
      setPageBodyDraft(pageId, html);
      setBodyHtml(html);
    },
    [pageId]
  );

  // The options menu's copy entry reads the live body; read-only pages
  // never mount here, so the registry exists only for editors (PAGE-044).
  React.useEffect(
    () => registerPageBodyActions(pageBodyActionsFor(() => handleRef.current?.html() ?? bodyHtml)),
    [bodyHtml]
  );

  // A pasted picture that carries an asset id from another page is cloned
  // onto this page before it lands (PAGE-039, bug NEWFRONT-351). The
  // handle reports the rewritten markup as an ordinary edit, so the save
  // path and the assets pane both see the new id.
  const onPasteHtml = React.useCallback(
    async (pasted: string): Promise<string | null> => {
      try {
        const clones = await clonePastedAssets(getAppClient(), { workspaceSlug, projectId, pageId }, pasted);
        return rewritePastedAssets(pasted, clones);
      } catch (error) {
        notifyError(pageImageUploadFailure(error));
        return null;
      }
    },
    [workspaceSlug, projectId, pageId]
  );

  // A pasted bare work-item key or browse address becomes an entity
  // reference; the resolver only rewrites when the search returns the
  // exact row, so an unknown key stays a normal link (PAGE-043).
  const onPasteText = React.useCallback(
    async (text: string): Promise<string | null> => {
      const parsed = parseEntityAddress(text);
      if (parsed === null) return null;
      try {
        const choice = await resolvePastedEntity(getAppClient(), workspaceSlug, projectId, parsed);
        if (choice === null) return null;
        return entityReferenceHtml(pageEntityReferenceFor(choice, workspaceSlug));
      } catch {
        return null;
      }
    },
    [workspaceSlug, projectId]
  );

  // Only a recognised work-item key/address is claimed; ordinary prose and
  // multi-line text keep the editor's native paste path (PAGE-043).
  const claimsPasteText = React.useCallback((text: string) => parseEntityAddress(text) !== null, []);

  const run = React.useCallback((command: PageFormatCommand) => {
    handleRef.current?.applyCommand(command);
  }, []);

  const pickBlock = React.useCallback((kind: PageBlockKind) => {
    handleRef.current?.applyBlockKind(kind);
  }, []);

  const pickColor = React.useCallback((kind: "text" | "background", key: string) => {
    handleRef.current?.applyColor(kind, key);
  }, []);

  const clearColor = React.useCallback((kind: "text" | "background") => {
    handleRef.current?.clearColor(kind);
  }, []);

  const upload = React.useCallback(
    async (file: File): Promise<UploadedPageImage> => {
      if (uploadImage !== undefined) return uploadImage(file);
      return uploadPageImage(getAppClient(), { workspaceSlug, projectId, pageId, file });
    },
    [uploadImage, workspaceSlug, projectId, pageId]
  );

  const pickImage = React.useCallback(
    (file: File) => {
      setUploading(true);
      void upload(file)
        .then((uploaded) => {
          handleRef.current?.insertImage(
            uploaded.assetUrl === "" ? pageAssetUrl(workspaceSlug, projectId, uploaded.assetId) : uploaded.assetUrl,
            uploaded.assetId,
            // The assets pane reads alt first; a bare UUID source would
            // otherwise be its row name. The old app's `image-<id>` label
            // keeps the row readable (PAGE-039).
            `image-${uploaded.assetId}`
          );
        })
        .catch((error: unknown) => {
          notifyError(pageImageUploadFailure(error));
        })
        .finally(() => {
          setUploading(false);
        });
    },
    [upload, workspaceSlug, projectId]
  );

  const handleKeyDown = React.useCallback((event: KeyboardEvent): boolean => {
    // The standard save gesture belongs to the page: capturing it keeps
    // the browser's save dialog closed whether or not a saver is mounted.
    if (isPageSaveGesture(event)) {
      event.preventDefault();
      saveRef.current?.();
      return true;
    }
    // Only the chords the editor leaves unbound (the alignments) run
    // here: ProseMirror skips its own key handler once a chord is
    // claimed, and an editor-native command returns null, so no chord
    // ever toggles twice.
    const command = pageKeyCommand(event);
    if (command === null) return false;
    event.preventDefault();
    handleRef.current?.applyCommand(command);
    return true;
  }, []);

  return (
    <>
      <PageCanvasHeader
        title={{
          pageId,
          name,
          editable: true,
          onRename,
          onDraftChange: setTitleDraft,
          onJumpToBody: () => handleRef.current?.focusStart(),
          collab,
          seedDocument,
        }}
        logoProps={logoProps}
        fontSize={fontSize ?? "large-font"}
        fontStyle={fontStyle ?? "sans-serif"}
        editable
        titleEmpty={titleIsEmpty(titleDraft)}
        onMarkPick={onMarkPick}
      />
      {showBar ? (
        <PageFormattingBar
          state={state}
          onCommand={run}
          onBlockKind={pickBlock}
          wide={fullWidth}
          mediaControls={
            <>
              <PageTableButton onInsert={() => handleRef.current?.insertTable()} />
              <PageImageButton onPick={pickImage} busy={uploading} inputId={PAGE_IMAGE_INPUT_ID} />
              <PageColorButton
                textColor={state.textColor}
                backgroundColor={state.backgroundColor}
                onPick={pickColor}
                onClear={clearColor}
              />
            </>
          }
        />
      ) : null}
      {oversized.visible ? <PageOversizedStrip onDismiss={oversized.dismiss} /> : null}
      <div className={`editor-container text-size-${fontSize ?? "large-font"} text-face-${fontStyle ?? "sans-serif"}`}>
        {bodyPending ? (
          <PageBodySkeleton />
        ) : (
          <React.Suspense
            fallback={<RichTextView html={initialHtml} label={PAGE_BODY_LABEL} resolveAsset={resolveAsset} />}
          >
            {/* The editor instance belongs to one page: a navigation to a
                sibling page opens its own document, never the old one. */}
            <PageBodyEditor
              key={pageId}
              initialHtml={initialHtml}
              resolveAsset={resolveAsset}
              workspaceSlug={workspaceSlug}
              onChange={onChange}
              onFormattingChange={setState}
              onReady={onReady}
              label={PAGE_BODY_LABEL}
              placeholder={PAGE_BODY_PLACEHOLDER}
              handleKeyDown={handleKeyDown}
              onPasteHtml={onPasteHtml}
              onPasteText={onPasteText}
              claimsPasteText={claimsPasteText}
              collab={collab}
              seedDocument={seedDocument}
            />
          </React.Suspense>
        )}
      </div>
      {/* The transient surfaces are a sibling of the body, never inside the
          measured `.editor-container`: the pane counts that container's
          blocks, so palette and mention rows must not lift the counts. They
          are fixed-positioned from the caret, so no wrapper is needed — and
          `.frame-renderer > .editor-container` stays the editor's own path. */}
      {bodyPending ? null : (
        <React.Suspense fallback={null}>
          <PageBodyInteractionLayer
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            getHandle={() => handleRef.current}
            html={bodyHtml}
            onPickImage={openPageImagePicker}
            onDocumentChange={setBodyHtml}
            assistantReady={assistantReady}
          />
        </React.Suspense>
      )}
    </>
  );
}
