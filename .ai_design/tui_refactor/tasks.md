# TUI Refactor — Task List

Tracks concrete implementation work for the design in `design.md`. Each task names files touched, has explicit acceptance criteria, and lists hard dependencies.

Conventions:

- **ID** format: `P<phase>-<n>`. Pre-work tasks: `PW-<n>`.
- **Status** legend: ` ` todo, `~` in progress, `x` done.
- A task is "done" only when its acceptance criteria pass _and_ `cargo build`, `cargo test`, `cargo clippy -- -D warnings` are clean from `runner/`.

---

## Pre-work (fill design-doc gaps)

These resolve open API questions before code lands. They are doc edits in `.ai_design/tui_refactor/design.md`. Land first.

- [x] **PW-1** — Add §5.12 _Per-tab contracts_. Spell out for each base tab (`GeneralView`, `RunnerStatusView`, `RunsView`, `ApprovalsView`): the `Pane` enum, the focused-child variants, owned child widgets (`TextArea` / `SelectableList<Id>`), and which `Context`s are active per pane. Acceptance: a Phase-3 implementer can write any tab struct without further questions.
- [x] **PW-2** — Add §5.13 _Type definitions_. Concrete signatures (no `…` placeholders) for: `Tui`, `View`, `Tab`, `KeyTarget`, `FocusedChild`, `KeymapRegistry`, `AppEvent` (full variant list), `AppEventSender`, `Cancellation`, `ViewCompletion`. Acceptance: every type referenced anywhere in §5 is declared exactly once with a stable signature.
- [x] **PW-3** — Add §5.14 _State-migration map_. Table mapping each field of today's `AppState` (`runner/src/tui/app.rs:76-145`) to its new home: per-view struct, on `App`, or deleted. Acceptance: no field is ambiguous.

---

## Phase 0 — Scaffolding (no behavior change)

Goal: every new module compiles, is unit-tested where applicable, and is unused by the running TUI. Lands as one PR (~600 LOC).

- [x] **P0-1** — Create `runner/src/tui/render/renderable.rs`. Implement `trait Renderable` (per §5.2), plus combinators `ColumnRenderable`, `FlexRenderable`, `InsetRenderable`. Implement for `()`, `&str`, `String`, `Line`, `Span`, `Paragraph`, `Option<R>`. Unit tests: layout math for each combinator. Depends on: PW-2.
- [x] **P0-2** — Create `runner/src/tui/event.rs` with the full `AppEvent` enum (variants per PW-2). No senders or handlers yet. Compiles standalone.
- [x] **P0-3** — Create `runner/src/tui/event_sender.rs` with `AppEventSender` (clone wrapper + convenience methods per PW-2). Unit test: send + receive round-trip.
- [x] **P0-4** — Create `runner/src/tui/widgets/scroll_state.rs`. Port `ScrollState` from `codex-rs/tui/src/bottom_pane/scroll_state.rs` (Apache-2.0; attribution comment). Unit tests: `clamp`, `move_up_wrap`, `move_down_wrap`, `ensure_visible` — port codex's tests verbatim.
- [x] **P0-5** — Create `runner/src/tui/widgets/selectable_list.rs`. Implement `SelectableList<Id: Eq + Clone>` with identity-stable `reconcile` (claudy `validatedFocusedValue` semantics). Unit tests: reconcile after item-add, item-remove, item-reorder, item-rename; selection survives all but rename, falls back to first on rename.
- [x] **P0-6** — Create `runner/src/tui/view/mod.rs`. Define `trait View`, `KeyHandled`, `Cancellation`, `ViewCompletion` per PW-2. No impls yet.
- [x] **P0-7** — Create `runner/src/tui/view/tab.rs`. Define `trait Tab` (or concrete BaseView shape per PW-2) including `focused_child()`, `active_contexts()`, `handle_key()`, `render()`. No impls yet.
- [x] **P0-8** — Create `runner/src/tui/input/keymap/mod.rs` with `Action` enum + `Context` enum (full lists per PW-2). Create `runner/src/tui/input/keymap/default_bindings.rs` with the binding table (Global, Tabs, List, Picker, ConfirmDialog, TextInput contexts). Create `runner/src/tui/input/keymap/resolver.rs` — pure `resolve(key, contexts, registry, pending) -> Resolution`. Unit tests: most-specific-context-wins, last-binding-wins, chord prefix match, chord cancel on Esc, null-action unbinding.
- [x] **P0-9** — Create `runner/src/tui/tui.rs` with `Tui` struct (per PW-2). `init()` enables raw mode + bracketed paste + focus events + panic hook (panic hook restores terminal). `Drop` impl restores terminal (`TerminalRestoreGuard` pattern).
- [x] **P0-10** — Create `runner/src/tui/tui/event_stream.rs`. Wrap `crossterm::EventStream` and emit `TuiEvent { Key, Paste, Resize, Draw }`. Filter `KeyEventKind::Release` at the stream boundary. Drop `Mouse` and `FocusLost`; map `FocusGained` to `Draw`. Unit tests with synthetic crossterm events.
- [x] **P0-11** — Create `runner/src/tui/tui/frame_rate_limiter.rs` and `runner/src/tui/tui/frame_requester.rs`. Port from codex (Apache-2.0; attribution). 120fps clamp via `MIN_FRAME_INTERVAL`. Coalescing tests: many `schedule_frame()` calls in one window produce one draw.
- [x] **P0-12** — Create `runner/src/tui/input/paste_burst.rs`. Vendor codex's pure state machine. Trim to what we need (single-line; no kill-buffer integration). Unit tests: HoldOrInsert, BeginBufferFromPending, BufferAppend, flush-before-modified-key, IME bypass.
- [x] **P0-13** — Acceptance check: `cargo build`+`cargo test`+`cargo clippy -- -D warnings` clean. Existing TUI binary unchanged. New modules under `runner/src/tui/` are reachable from `lib.rs` (declared modules) but unused by `app.rs`.

---

## Phase 1 — Event loop + input pipeline (the bug-fix phase)

Goal: real fix for Bug 3 (digits in form fields no longer tab-switch). Lands as one PR.

- [x] **P1-1** — Add `runner/src/tui/widgets/textarea.rs` (single-line). Owns `String` + `cursor: usize`. Methods: `insert_char`, `delete_backward`, `move_left`, `move_right`, `clear`, `cursor_pos`, `render(area, buf, focused: bool)`. Filters `KeyEventKind::Release` at its own boundary (defense in depth). Unit tests + buffer-snapshot tests. Depends on: P0-1, P0-12.
- [x] **P1-2** — Implement `runner/src/tui/views/general.rs` as a `Tab` impl with the inline register form rebuilt from real `TextArea` widgets (one per field). Owns its `focused_field` enum. The textarea is the focused child during editing; `active_contexts()` returns `[Context::TextInput]` only — `Tabs` and `Global` are excluded. Buffer-snapshot tests for empty / partially-filled / error states. Depends on: P1-1, PW-1.
- [x] **P1-3** — Replace `app::loop_ui` with the three-source `select!` from §5.3: `app_event_rx`, `tui_events.next()`, `ticker.tick()`. Implement `app::dispatch_app_event` (flat match) and `app::handle_tui_event` (Resize/Draw/Paste/Key dispatch). Implement `app::dispatch_key` enforcing the five-layer routing order from §5.5. Depends on: P0-2, P0-3, P0-9, P0-10, P0-11, P1-2.
- [x] **P1-4** — Move IPC off the input path per §5.7. Add `IpcInFlight { status: bool, approvals: bool, runs: bool }` on `App`. On `AppEvent::Tick`, `dispatch_app_event` spawns one-shot tasks per concern (gated by in-flight flags + `Tab::Runs` for runs). Tasks post `AppEvent::Ipc{Status,Approvals,Runs}Updated` results back. `app::refresh()` is deleted. Depends on: P1-3.
- [x] **P1-5** — Replace per-loop `terminal.draw(...)` with `FrameRequester`-driven render: `TuiEvent::Draw` triggers `tui.draw(|f| app.render(f))`. Every state-mutating handler ends with `frame.schedule_frame()`. Delete `print!("\x07")` mid-render bell; replace with a deferred `AppEvent::Bell` posted after the next draw. Depends on: P0-11, P1-3.
- [x] **P1-6** — Wire only the General tab to the new pipeline; other tabs still call into legacy `handle_event` via a temporary bridge until Phase 3. Acceptance criteria for Bug 3 are local to General. Depends on: P1-2, P1-3.
- [x] **P1-7** — **Acceptance gate**:
  - Type `1234567890` into Cloud URL/hostname/token while register form is focused — all 10 chars inserted; no tab switches.
  - Type `hl` into a hostname field — both characters inserted, not tab navigation.
  - Paste a multi-line URL into Cloud URL — inserted as one chunk.
  - Tab/Shift+Tab still switch tabs from non-text-input panes.
  - `cargo test` green; `cargo clippy -- -D warnings` clean.

---

## Phase 2 — View stack + remaining modals

Goal: every modal opens / closes via the stack; global hotkeys disabled while a modal is open.

- [x] **P2-1** — Implement `App::push_view` / `App::pop_with_completion` per §5.4. Add `view_stack: Vec<Box<dyn View>>` to `App`. Render after base; cursor delegated to top of stack else base. Unit tests with mock `View` impls.
- [x] **P2-2** — Convert `confirm_exit` and `confirm_stop` to `ConfirmView` impls in `runner/src/tui/views/modals/confirm.rs`. Single shared component parameterized by message + Yes/No callbacks. Buffer-snapshot tests.
- [x] **P2-3** — Convert `help` to `HelpView` impl in `runner/src/tui/views/modals/help.rs`. Buffer-snapshot tests.
- [x] **P2-4** — Convert `remove_runner_confirm` to a `ConfirmView` parameterization (reuse P2-2).
- [x] **P2-5** — Convert `add_runner_form` to `AddRunnerView` impl in `runner/src/tui/views/modals/add_runner.rs`. Owns its `TextArea`s and `SelectableList<RunnerId>` for the picker submodal. Buffer-snapshot tests.
- [x] **P2-6** — Delete the corresponding modal early-returns from the legacy `handle_event`. The legacy function shrinks to "handle keys for tabs that haven't been migrated yet."
- [x] **P2-7** — **Acceptance gate**: every modal opens (via the action that previously opened it), closes on `Esc` (`on_ctrl_c` returns Handled with completion=Cancelled), commits on Enter. Global hotkeys (`q`, digit keys) are inert while a modal is open.

---

## Phase 3 — Per-view state, kill the flat `AppState`

Goal: each tab is a struct in `runner/src/tui/views/`, owning its own list state, focused child, and pane state.

- [x] **P3-1** — Implement `runner/src/tui/views/runs.rs` as a `Tab` impl. Owns `SelectableList<RunId>` reconciled from `App.runs`. No `state.selected` involvement. Buffer-snapshot tests.
- [x] **P3-2** — Implement `runner/src/tui/views/approvals.rs` as a `Tab` impl. Owns `SelectableList<ApprovalId>` for pending + a `DetailPane` for the right side. `focused_pane: Pane` enum (Pending | Detail). Buffer-snapshot tests for both panes focused.
- [x] **P3-3** — Implement `runner/src/tui/views/runner_status.rs` as a `Tab` impl. Owns `SelectableList<RunnerId>` for the list card + a `SettingsPane` for the right card. `[d]` removal reads `list.selected_id()`. Delete the `runner_picker_idx` / `runners_list_idx` aliasing.
- [x] **P3-4** — Delete from `AppState` per PW-3: `selected`, `runner_picker_idx`, `runners_list_idx`, `tab_general_field`, `runner_tab_focus`, `confirm_exit`, `confirm_stop`, `confirm_exit_yes`, `confirm_stop_yes`, `add_runner_form`, `register_form`, `config_edit_buffer`, `remove_runner_confirm`, `help`. Whatever remains on `App` is genuinely cross-tab state (IPC snapshots, ticker handles, frame requester, event sender).
- [x] **P3-5** — Delete the legacy `handle_event` and `refresh` functions. The bridge from Phase 1 disappears.
- [x] **P3-6** — **Acceptance gate**:
  - On Approvals with 50+ items: scroll past the visible window, wait 500ms tick — selection retained, scroll offset retained.
  - On Runners: `[d]` removes the highlighted row, not row 0.
  - Switching tabs preserves each tab's own selection (no global reset to 0).
  - Visual focus indicator (border style) follows the focused pane on every tab.

---

## Phase 4 — Paste burst integration into textareas

Goal: pasting into form fields works on terminals without bracketed paste; embedded Enter doesn't submit mid-paste.

- [x] **P4-1** — Wire `paste_burst` (P0-12) into `TextArea` (P1-1). On every key event, the textarea's parent (the active tab) routes through the paste-burst state machine first. Burst flush is scheduled via `FrameRequester::schedule_frame_in(burst_window)`.
- [x] **P4-2** — Implement paste-blocks-Enter rule (claudy `BaseTextInput.tsx:59-66`): while the burst buffer is non-empty, Enter is dropped instead of submitting.
- [x] **P4-3** — Bracketed-paste path (`TuiEvent::Paste`) bypasses the burst machine entirely and inserts directly into the focused textarea.
- [x] **P4-4** — **Acceptance gate**:
  - Rapid pasted text into hostname (no bracketed paste support) — inserted as one chunk.
  - Bracketed paste (real terminal) — inserted as one chunk.
  - Pasted text containing `\n` does not submit mid-paste.
  - Non-ASCII / IME chars insert directly without buffering.

---

## Phase 5 — Declarative keymap migration (cleanup)

Goal: every hotkey lives in `default_bindings.rs`; ad-hoc match arms are gone.

- [x] **P5-1** — Migrate Phase 1's hand-rolled General-tab key handling onto the keymap. Each pane returns its `active_contexts()` from a small method.
- [x] **P5-2** — Migrate Phase 2's modal key handling onto the keymap (`ConfirmDialog`, `Picker` contexts).
- [x] **P5-3** — Migrate Phase 3's tab-level handling onto the keymap.
- [x] **P5-4** — Audit: `grep -rn "KeyCode::" runner/src/tui` should show only the textarea, the keymap resolver, and the binding table. Nothing else.
- [x] **P5-5** — **Acceptance gate**: every hotkey works as before; rebinding any single action requires editing only `default_bindings.rs`.

---

## Optional follow-ups (not part of this refactor)

- [ ] **F-1** — User-overridable bindings (`~/.config/runner/keybindings.toml`) — claudy `loadUserBindings.ts` shape.
- [ ] **F-2** — `Option<Overlay>` for full-screen views (e.g. log pager).
- [ ] **F-3** — Multi-line `TextArea` (when free-text approval comments are added).
- [ ] **F-4** — `Tui::with_restored` for an external-editor flow.

---

## Smoke checklist (run after every phase ≥ Phase 1)

Quick manual pass — keeps a regression from sliding through automated tests.

1. Type `1234567890` into a focused text field — all 10 chars inserted; no tab switches.
2. Tab between cards on every tab — visual focus indicator follows.
3. Open Approvals with ≥ 20 items, scroll past the visible window, wait one tick — selection retained, scroll retained.
4. Paste a multi-line URL into Cloud URL — inserted as one chunk; Enter inside paste does not submit.
5. Open any modal → `Esc` closes; global keys (`q`, digit keys) inert while open.
6. Resize the terminal during a refresh — no panic, no corrupted glyphs, no lost selection.
