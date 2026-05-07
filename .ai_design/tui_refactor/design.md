# TUI Refactor — Design Doc

**Status:** Draft, not yet approved
**Owner:** runner / TUI
**Scope:** `runner/src/tui/**`
**Touches:** `runner/src/tui/app.rs`, `runner/src/tui/views/*`, `runner/src/tui/widgets/*`, plus new modules below
**Reference codebases:**

- `codex-rs/tui` — `/home/rich/dev/study/codex/codex-rs/tui/src/` (Rust + ratatui + crossterm — same stack as us)
- `claudy` — `/home/rich/dev/study/claudy/src/` (TS + forked Ink — different stack, identical input-handling discipline)

---

## 0. Reading order

This document has two parts:

- **Part I — Architecture lifted from codex-rs and claudy.** A self-contained chapter that maps both reference architectures, distills the design rules, and proposes the target architecture for the runner TUI. Read this first.
- **Part II — Migration plan, bug mapping, testing, risks.** How we get from today's TUI to the target. Read this when ready to execute.

The bugs that prompted this refactor (focus, selection, swallowed chars) are real but they are _symptoms_ of a missing architecture. Part I sets the architecture; Part II uses it to fix the bugs.

---

# Part I — Architecture

## 1. Why these two references

Two TUIs in our environment do this well, in different stacks:

- **codex-rs/tui** is `ratatui` + `crossterm` + `tokio` — exactly our stack. Its architecture is layered, bus-driven, and rigorously separates _terminal physics_ from _application logic_. We can port its shape almost line-for-line.
- **claudy** is TS + a forked Ink. The _interaction discipline_ it enforces (focus is derived not stored; one stdin reader; refs for race-prone state; declarative keybindings with scoped contexts; mount-order matters) is language-neutral. We port the discipline, not the code.

Both arrived at the same answers for the hard problems: _what is a focused widget?_, _who owns the terminal?_, _how does input not get dropped?_, _how is a modal opened?_. Where they agree, we copy. Where one is more developed (codex's `Renderable` trait family; claudy's keybinding resolver), we copy from the leader.

---

## 2. codex-rs architecture in one chapter

Excerpts and citations are from `/home/rich/dev/study/codex/codex-rs/tui/src/`.

### 2.1 Seven layers

```
┌──────────────────────────────────────────────────────────────────┐
│ L1  Process entry         main.rs, lib.rs::run_main              │
├──────────────────────────────────────────────────────────────────┤
│ L2  Terminal owner        tui.rs (Tui), tui/event_stream.rs,     │
│                           tui/frame_requester.rs,                │
│                           custom_terminal.rs                     │
├──────────────────────────────────────────────────────────────────┤
│ L3  App orchestrator      app.rs (App), app/event_dispatch.rs,   │
│                           app_event.rs (bus), app_event_sender,  │
│                           pager_overlay.rs                       │
├──────────────────────────────────────────────────────────────────┤
│ L4  Per-thread widget     chatwidget.rs, history_cell.rs,        │
│                           streaming/, exec_cell/                 │
├──────────────────────────────────────────────────────────────────┤
│ L5  Bottom pane           bottom_pane/mod.rs (BottomPane),       │
│                           bottom_pane/bottom_pane_view.rs,       │
│                           list_selection_view, approval_overlay  │
├──────────────────────────────────────────────────────────────────┤
│ L6  Composer + popups     chat_composer.rs, textarea.rs,         │
│                           paste_burst.rs, command_popup.rs,      │
│                           selection_popup_common.rs              │
├──────────────────────────────────────────────────────────────────┤
│ L7  Render primitives     render/renderable.rs (trait),          │
│                           wrapping.rs, live_wrap.rs              │
└──────────────────────────────────────────────────────────────────┘
```

Dependencies flow strictly downward. Communication upward goes through one of two channels: emit an `AppEvent` (the bus), or return a value (`InputResult`, `ViewCompletion`) up the call stack. **Layers never reach across — a popup never calls into `App`; it sends an `AppEvent` and the dispatcher re-enters from the top.**

### 2.2 Lifecycle

`main.rs` → `lib::run_main` → terminal init → `App::run` → cleanup. Concretely:

1. `lib::run_main` (`lib.rs:678`) loads config, runs login/onboarding, brings up the app-server, then calls `tui::init` + `Tui::new`.
2. `tui::init` (`tui.rs:272-288`) sets `EnableBracketedPaste` + raw mode + keyboard-enhancement flags + `EnableFocusChange`, flushes stdin, installs a panic hook (`tui.rs:290-296`) so the terminal restores even on panic, and constructs a `CrosstermBackend<Stdout>` wrapped in codex's `custom_terminal::Terminal` (inline-viewport semantics).
3. `Tui::new` (`tui.rs:335-368`) creates the `(broadcast<()>(1), FrameRequester)` pair and `tokio::spawn`s the `FrameScheduler` actor.
4. `App::run` (`app.rs:659-1109`) creates the unbounded `(app_event_tx, app_event_rx)` channel, wires `AppEventSender`, builds `ChatWidget`, requests the first frame, and falls into the `select!` loop.
5. Shutdown is **layered**: cooperative `AppEvent::Exit` returns `AppRunControl::Exit(_)` from the loop → `app_server.shutdown().await` → `tui.terminal.clear()`. RAII bottom layer is `TerminalRestoreGuard` (`lib.rs:1604-1634`); the panic hook does the same — even a panic restores the terminal.

The pause primitive is `Tui::with_restored(mode, f)` (`tui.rs:412-444`):

```rust
self.pause_events();                   // drop crossterm EventStream
let was_alt = self.is_alt_screen_active();
if was_alt { self.leave_alt_screen(); }
mode.restore()?;                       // disable raw, bracketed-paste, …
let output = f().await;
set_modes()?;
flush_terminal_input_buffer();
if was_alt { self.enter_alt_screen(); }
self.resume_events();                  // recreate crossterm EventStream
output
```

The drop-and-recreate of the crossterm `EventStream` (rather than just pausing) is mandatory — comment at `tui/event_stream.rs:9-18` explains crossterm's stdin reader thread keeps reading bytes otherwise, racing with the launched external program.

### 2.3 The `AppEvent` bus

`AppEvent` (`app_event.rs:120-805`) is a 150-variant enum. Doc comment (`app_event.rs:1-9`):

> "AppEvent is the internal message bus between UI components and the top-level App loop. Widgets emit events to request actions that must be handled at the app layer, without needing direct access to App internals."

Variants fall into seven groups: lifecycle, op-forwarding, history mutation, streaming animation, background result, settings persistence, UI-popup intent.

`AppEventSender` (`app_event_sender.rs:18-115`) wraps `mpsc::UnboundedSender<AppEvent>` and adds: session-log integration on every send (`l31-37`), convenience methods that wrap common variants (`interrupt()`, `compact()`, `exec_approval(…)`), and failure-swallowing with logging. It is `Clone` and handed to every widget at construction. **Widgets never see the App struct**; they hold a sender.

The dispatcher (`app/event_dispatch.rs:12-1966`) is a flat giant match. It is intentionally flat — the dispatcher is the index of who-handles-what; domain logic lives in `app/session_lifecycle.rs`, `app/thread_routing.rs`, etc.

**Why a bus instead of method calls?** Two reasons (`app_event.rs:184-185`): channel-of-channels avoidance (without the bus every widget needs a `Sender<DomainEvent>` per subsystem); and _linearization with thread-buffered events and async results_ — putting everything on one queue establishes a single happens-before order so `InsertHistoryCell` and `ApplyThreadRollback` interleave deterministically.

### 2.4 The four-source `select!`

`app.rs:1020-1068`:

```rust
loop {
    let control = select! {
        Some(event) = app_event_rx.recv() =>
            app.handle_event(...).await,

        active = async { … }, if should_handle_active_thread_events(...) =>
            app.handle_active_thread_event(...).await,

        event = tui_events.next() => match event {
            Some(ev) => app.handle_tui_event(...).await,
            None     => app.handle_exit_mode(ExitMode::ShutdownFirst).await,
        },

        srv = app_server.next_event(), if listen_for_app_server_events => …,
    };
}
```

| Source                    | Producer                                 | Gate                              |
| ------------------------- | ---------------------------------------- | --------------------------------- |
| `app_event_rx`            | every widget + every background task     | always                            |
| `active_thread_rx`        | per-thread listener task                 | until initial `SessionConfigured` |
| `tui_events.next()`       | `TuiEventStream` (key/paste/resize/draw) | until stdin EOF                   |
| `app_server.next_event()` | embedded JSON-RPC stream                 | `false` once the stream closes    |

Starvation is avoided by (a) `tokio::select!`'s default biased-fair polling, and (b) the `TuiEventStream`'s explicit round-robin between draw broadcasts and crossterm input (`tui/event_stream.rs:265-291`).

**Why two event types?** `TuiEvent` is _terminal-physical_ (Key/Paste/Resize/Draw). `AppEvent` is _application-logical_. They have independent cadences (frame coalescing belongs to the terminal layer, not the app). Backtrack overlays intercept `TuiEvent`s without contaminating `AppEvent`'s shape (`app.rs:1127-1128`).

### 2.5 The `Renderable` trait family

Three nested traits, each adding capability.

**`Renderable`** (`render/renderable.rs:13-19`) — the lowest layer:

```rust
pub trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16;
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> { None }
}
```

`desired_height` is _width-aware_ — it lets the terminal layer measure widget height before sizing the inline viewport. Implemented for `()`, `&str`, `String`, `Span`, `Line`, `Paragraph`, `Option<R>`, `Arc<R>`, plus composition adapters: `ColumnRenderable`, `FlexRenderable`, `RowRenderable`, `InsetRenderable` (which themselves implement `Renderable`, so layouts compose without runtime virtual dispatch overhead — one box at the leaf).

**`BottomPaneView`** (`bottom_pane/bottom_pane_view.rs:18-133`) — extends `Renderable`:

```rust
pub(crate) trait BottomPaneView: Renderable {
    fn handle_key_event(&mut self, _key_event: KeyEvent) {}
    fn is_complete(&self) -> bool { false }
    fn completion(&self) -> Option<ViewCompletion> { None }
    fn dismiss_after_child_accept(&self) -> bool { false }
    fn on_ctrl_c(&mut self) -> CancellationEvent { CancellationEvent::NotHandled }
    fn prefer_esc_to_handle_key_event(&self) -> bool { false }
    // … plus paste_burst, approval try-consume, terminal-title hooks
}
```

It adds: a _completion model_ (`is_complete()` + `completion() -> ViewCompletion::{Accepted, Cancelled}`); _Esc routing policy_ (`prefer_esc_to_handle_key_event` flips Esc from cancellation to in-view handling); _try-consume_ of approval/elicitation requests so an open view can absorb them.

Children are **owned, not borrowed**: `view_stack: Vec<Box<dyn BottomPaneView>>` (`bottom_pane/mod.rs:199`). The pane's `as_renderable` returns either `RenderableItem::Borrowed(view)` (when a view is on the stack) or a `FlexRenderable` of static panels. **Cursor position propagates upward by composition**: composer's `TextArea` reports its caret → `ChatComposer::cursor_pos` → `BottomPane::cursor_pos` → `ChatWidget::cursor_pos` → `App` → `frame.set_cursor_position`.

### 2.6 Modality lives in two stacks

- **`BottomPane::view_stack: Vec<Box<dyn BottomPaneView>>`** — _inline_ modals owned by the chat surface (composer-region). Examples: model picker, approval prompt.
- **`App::overlay: Option<Overlay>`** — _full-screen_ alt-screen overlays owned by the app. Examples: transcript pager (`Ctrl+T`), diff viewer.

Pushing onto `view_stack` happens via `push_view(Box<dyn BottomPaneView>)` (`mod.rs:447-450`). **There is no explicit pop.** After every key, the pane checks `view.is_complete()`; if true, `pop_active_view_with_completion(view.completion())` runs (`mod.rs:452-473`):

```rust
match completion {
    Some(Accepted)  => while view_stack.last().is_some_and(|v| v.dismiss_after_child_accept()) {
                          view_stack.pop();
                       },
    Some(Cancelled) => if let Some(v) = view_stack.last_mut() { v.clear_dismiss_after_child_accept(); },
    None            => {}
}
```

So when a deeply-nested confirmation accepts, all parent menus that opted into "tear down on child accept" disappear in one step; on cancel only the one popped is removed.

Input routing branches on `overlay.is_some()` first (`app.rs:1127`), then on `view_stack.is_empty()` second. Mixing the two stacks is structurally prevented.

### 2.7 The composer (text input subsystem)

Layered handlers in `ChatComposer::handle_key_event` (`bottom_pane/chat_composer.rs:1548`):

```
ChatComposer::handle_key_event
├── if !input_enabled              → return
├── if KeyEventKind::Release       → return
├── if history_search.is_some()    → handle_history_search_key
├── if is_history_search_key(prev) → begin_history_search
└── match active_popup:
    ├── Command → handle_key_event_with_slash_popup
    ├── File    → handle_key_event_with_file_popup
    ├── Skill   → handle_key_event_with_skill_popup
    └── None    → handle_key_event_without_popup
                  └── handle_input_basic
                      ├── flush paste_burst if due
                      ├── if disable_paste_burst OR Ctrl/Alt → textarea.input(event)
                      └── else paste_burst.on_plain_char(...) (HOLD/BUFFER/PASS)
```

`KeyEventKind::Release` is filtered at _every_ layer: `bottom_pane/mod.rs:531-534`, `chat_composer.rs:1553-1555` ("Ignore key releases here to avoid treating them as additional input … via paste-burst logic"), `chat_composer.rs:3098-3100`, `textarea.rs:331-336`. This redundancy is the feature: any single layer added later cannot reintroduce duplicate-character bugs on Windows / Kitty protocol.

`TextArea` (`bottom_pane/textarea.rs:90-99`) owns: text + cursor, wrap cache, `elements` (placeholder ranges that move atomically with edits), `kill_buffer`, `editor_keymap`. It deliberately does **not** do: paste-burst detection, popup management, slash-command parsing, history. Comment at `textarea.rs:1-11` is explicit: "pure editor."

`paste_burst.rs` is a **pure state machine** — it never mutates the textarea. It returns decisions: `RetainFirstChar | BeginBufferFromPending | BeginBuffer { retro_chars } | BufferAppend`. The caller applies the edits. Why exist? Terminals without bracketed paste (Windows console, VS Code integrated, some SSH paths) deliver pastes as a flood of `KeyCode::Char` events; without this, embedded `Enter` would submit mid-paste, bound shortcuts (`?`, `1`, `h`) would fire on pasted text. Critical detail: **flush before any modified key** (`chat_composer.rs:3092-3094`) — forgetting this leaves buffered text waiting forever and feels like "swallowed chars."

### 2.8 Frame scheduling

`FrameRequester` (handle, `Clone`) + `FrameScheduler` (actor, `tokio::spawn`-ed) — `tui/frame_requester.rs`. Multiple `schedule_frame()` calls within one frame interval merge into the _earliest_ deadline; only when sleep fires does a single `()` go out on `draw_tx`, appearing as one `TuiEvent::Draw` in the main `select!`.

```rust
loop {
    select! {
        draw_at = receiver.recv() => {
            let draw_at = self.rate_limiter.clamp_deadline(draw_at);
            next_deadline = Some(next_deadline.map_or(draw_at, |cur| cur.min(draw_at)));
        }
        _ = sleep_until(target) => {
            if next_deadline.is_some() {
                next_deadline = None;
                rate_limiter.mark_emitted(target);
                let _ = self.draw_tx.send(());
            }
        }
    }
}
```

`schedule_frame_in(dur)` is used by paste-burst flush so even without further keystrokes the burst window will eventually expire and convert to a paste. The 120fps clamp is `MIN_FRAME_INTERVAL = 8.33 ms` (`tui/frame_rate_limiter.rs:13`).

### 2.9 Background work

Lifetime is owned by **channel/handle ownership**, not `CancellationToken`s:

| Task                       | Spawned in                          | Tied to App lifetime by              | Talks back via                     |
| -------------------------- | ----------------------------------- | ------------------------------------ | ---------------------------------- |
| `FrameScheduler::run`      | `FrameRequester::new`               | drops with `Tui` (holds the sender)  | `broadcast<()>` → `TuiEvent::Draw` |
| Per-thread listener        | `App::run` (HashMap of JoinHandles) | `JoinHandle` retained in App         | per-thread mpsc bridge             |
| File-search session        | `FileSearchManager`                 | `Arc<Mutex<…>>`; recreated per query | `AppEvent::FileSearchResult`       |
| App-server background reqs | `app/background_requests.rs`        | clonable `AppServerRequestHandle`    | `AppEvent::*Loaded` variants       |

Cancellation is implicit: drop the sender, the task observes `None` and exits. The architecture _aggressively avoids_ futures-cancellation primitives because they create surprising drop-order bugs around terminal state.

### 2.10 The seven Big Ideas (codex)

1. **Render is a pure function of widget state.** Every renderable is `(area, buf) -> ()`, `(width) -> u16`, `(area) -> Option<(u16,u16)>`. Render never mutates, never schedules, never sends events.
2. **Mutation goes through the bus.** Producers fire `AppEvent`s; the dispatcher is one giant match. Widgets never call App methods or each other's methods. Single event ordering = deterministic interleaving.
3. **The terminal is owned by exactly one struct.** `Tui` is the only thing touching `stdout`, raw mode, alt-screen, focus events, frame scheduling. App + below ask `Tui` to do things; never reach past it.
4. **Two event layers, not one.** `TuiEvent` (Key/Paste/Resize/Draw) is _physical_. `AppEvent` is _logical_. Frame scheduler exists because draw cadence is independent of event cadence.
5. **Modality lives in two stacks.** `view_stack` for inline modals, `Option<Overlay>` for full-screen. Each has its own Esc/cancel semantics. Mixing is structurally prevented.
6. **Streams of bytes, not commands.** History cells are `Arc<dyn HistoryCell>`. The transcript is a `Vec<Arc<dyn HistoryCell>>`. "The transcript at frame N" is a stable snapshot — copyable, re-flowable on resize, never rebuilt from raw deltas.
7. **No CancellationTokens; ownership is the lifetime.** Background tasks die when their senders/handles drop.

---

## 3. claudy architecture in one chapter

Excerpts and citations from `/home/rich/dev/study/claudy/src/`.

### 3.1 Layers

```
┌──────────────────────────────────────────────────────────────┐
│ L7  Entrypoints      entrypoints/, replLauncher.tsx          │
├──────────────────────────────────────────────────────────────┤
│ L6  Screen shell     components/App.tsx, screens/REPL.tsx    │
├──────────────────────────────────────────────────────────────┤
│ L5  Cross-cutting    keybindings/, context/, state/, hooks/  │
├──────────────────────────────────────────────────────────────┤
│ L4  UI widgets       components/PromptInput, CustomSelect,   │
│                      BaseTextInput, dialogs                  │
├──────────────────────────────────────────────────────────────┤
│ L3  Domain logic     query.ts, tools/, tasks/, coordinator/  │
├──────────────────────────────────────────────────────────────┤
│ L2  Renderer         ink/  (forked Ink: reconciler,          │
│                      parse-keypress, renderer, hooks)        │
├──────────────────────────────────────────────────────────────┤
│ L1  Process / I/O    stdin (raw), bracketed paste, kitty     │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 Single stdin reader

`ink/components/App.tsx:332` — `handleReadable` is the **only** `'readable'` listener on stdin. The early-input capture set up in `entrypoints/cli.tsx:288-298` is _destructively_ removed before this listener attaches (`App.tsx:222-228`). `usePasteHandler.ts:208-212` quotes the bug that motivated the rule:

> "Paste detection is now done via the InputEvent's keypress.isPasted flag … This avoids the race condition caused by having multiple listeners on stdin. Previously, we had a stdin.on('data') listener here which competed with the 'readable' listener in App.tsx, causing dropped characters."

**Two stdin readers drop bytes.** This is the first rule; everything else builds on it.

### 3.3 Keybinding system — the part most worth porting

**Data model** (`keybindings/defaultBindings.ts:32-340`):

```ts
{ context: 'Chat', bindings: { 'enter': 'chat:submit', 'up': 'history:previous', ... } }
```

Each block names a `KeybindingContextName` and maps a chord-string (`'ctrl+x ctrl+e'` — space introduces a chord step) to an action. `null` action _unbinds_ the chord. User overrides loaded from `~/.claude/keybindings.json`; merge rule is **last-binding-wins**.

**Resolution** (`keybindings/resolver.ts:166-244`) — pure function:

```ts
type ChordResolveResult =
  | { type: 'match'; action: string }
  | { type: 'none' }
  | { type: 'unbound' }
  | { type: 'chord_started'; pending: ParsedKeystroke[] }
  | { type: 'chord_cancelled' }

resolveKeyWithChordState(input, key, activeContexts, bindings, pending) → ChordResolveResult
```

Algorithm:

1. Escape mid-chord cancels the chord.
2. Build current keystroke from Ink's `(input, key)`.
3. `testChord = pending ? [...pending, current] : [current]`.
4. Filter bindings by active contexts.
5. Look for chord prefixes (longer chords whose action is non-null) → `chord_started`.
6. Look for exact match, **last one wins** → `match | unbound`.
7. Else if mid-chord → `chord_cancelled`; otherwise → `none`.

**Conflict rules** (`useKeybinding.ts:54-60`):

```ts
const contextsToCheck = [
  ...keybindingContext.activeContexts,  // most-specific
  context,                               // caller's context
  'Global',                              // fallback
]
new Set([...])  // dedup, preserve order
```

Most-specific wins; within a context, last-binding-wins.

**Hook surface:**

- `useRegisterKeybindingContext(name, isActive)` — RAII via `useLayoutEffect`; adds the context to `activeContexts` for the lifetime of the component.
- `useKeybinding(action, handler, { context, isActive })` — registers handler; on `'match'` invokes it; if handler returns non-`false`, calls `event.stopImmediatePropagation()`.
- **`Consumed | NotConsumed`** convention: handler returning `false` means _not consumed_ — propagation continues. Comment at `useKeybinding.ts:113-121`: "Useful for fall-through: e.g. ScrollKeybindingHandler's scroll:line\* returns false when the ScrollBox content fits, letting a child component's handler take the wheel event for list navigation instead."

### 3.4 Focus is derived, not stored

Canonical example — `components/PromptInput/PromptInput.tsx:2199`:

```ts
focus: !isSearchingHistory && !isModalOverlayActive && !footerItemSelected;
```

Three orthogonal signals collapsed to one boolean. None of them is "the focused widget." There is **no focus pointer anywhere in the tree.**

The signals' provenances:

- `isSearchingHistory` — local `useState`, owned by `PromptInput`.
- `isModalOverlayActive` — `useIsModalOverlayActive()` reads `AppState.activeOverlays` and filters out the `NON_MODAL_OVERLAYS = new Set(['autocomplete'])` allowlist (`overlayContext.tsx:21,140-150`).
- `footerItemSelected` — global `AppState`, set by footer-chip dialogs.

**Overlay registry** (`context/overlayContext.tsx:38-104`):

```ts
function useRegisterOverlay(id: string, enabled: boolean = true) {
  useEffect(() => {
    if (!enabled) return;
    addOverlay(id);
    return () => removeOverlay(id); // RAII: cleanup removes id
  }, [id, enabled]);
}
```

A modal popping open mounts an overlay component, which calls `useRegisterOverlay(id, true)` in its body. The id appears in `AppState.activeOverlays`. The text input's `focus` prop becomes `false` because `isModalOverlayActive` is now true. No `focus()` method is ever called. When the overlay unmounts, the cleanup removes the id — focus _returns_ by data flow, not by an explicit "restore focus" code path.

**Hardware cursor parking** — `useDeclaredCursor` (`ink/hooks/use-declared-cursor.ts:25-73`): each frame, exactly one widget declares "the cursor goes here." Inactive widgets clear only if the currently-declared node is theirs (node-identity check defends against memoized active siblings and sibling-handoff during focus moves).

### 3.5 CustomSelect — the selection widget

File breakdown of `components/CustomSelect/`:

- `select.tsx` (690 lines) — main component
- `use-select-state.ts` (158 lines) — owns `value` (accepted)
- `use-select-navigation.ts` (654 lines) — owns `focusedValue` (cursor)
- `use-select-input.ts` (288 lines) — wires keybindings + raw `useInput`
- `option-map.ts` (52 lines) — doubly-linked-map for O(1) prev/next

**Two reducers** — claudy splits _cursor_ (`focusedValue`) from _selection_ (`value`):

- `useSelectNavigation`: `focusedValue`, page math, OptionMap. Actions: `focus-next-option`, `focus-previous-option`, …
- `useSelectState`: `useState<T | undefined>(defaultValue)` for `value`. Only `selectFocusedOption()` writes (`use-select-state.ts:146-148`).

**Identity-stable selection** — `use-select-navigation.ts:592-602`:

```ts
const validatedFocusedValue = useMemo(() => {
  if (state.focusedValue === undefined) return undefined;
  const exists = options.some((opt) => opt.value === state.focusedValue);
  if (exists) return state.focusedValue;
  return options[0]?.value;
}, [state.focusedValue, options]);
```

Data refresh can change `options`. Without this memo, the cursor would point at a value that no longer exists. With it, the cursor falls back to first-option until next reducer dispatch reconciles.

**Wrap-around defaults; opt-out via callbacks** (`use-select-input.ts:117-124`): if the parent provides `onDownFromLastItem`, hitting Down at the bottom invokes the parent (navigation "bleeds" to a sibling widget) instead of wrapping.

**Input-row escape hatch** (`use-select-input.ts:115-148`): when the focused option is `type: 'input'`, the keybinding handlers for `select:next/previous/accept` are **not registered at all** — `j/k/enter` pass through to the `BaseTextInput` embedded in the row. By omission, not by `if/else`.

**Navigation declarative; contextual keys imperative** — `select:next/previous/accept/cancel` are keybindings; digit shortcuts (`1`–`9`), `pageUp/pageDown`, Tab (toggle input mode), Space (multiselect toggle) stay in raw `useInput`. Rule of thumb: if a user might want to remap it, it's an action. Otherwise it's intrinsic widget UX.

### 3.6 Refs for race-prone state

Two canonical examples both with verbatim explanatory comments:

`usePasteHandler.ts:48-53`:

> "Mirrors pasteState.timeoutId but updated synchronously. When paste + a keystroke arrive in the same stdin chunk, both wrappedOnInput calls run in the same discreteUpdates batch before React commits — the second call reads stale pasteState.timeoutId (null) and takes the onInput path. If that key is Enter, it submits the old input and the paste is lost."

`KeybindingProviderSetup.tsx:135-142`:

> "Chord state management — use ref for immediate access, state for re-renders. The ref is used by resolve() to get the current value without waiting for re-render. The state is used to trigger re-renders when needed."

The pattern: **ref = source of truth, state = render trigger.** A single `setPendingChord` wrapper writes both in one call.

In Rust this maps to `Cell<Option<…>>` / `AtomicBool` / `RefCell` — interior mutability for state that must be readable inside the same tick it was written.

### 3.7 Mount order = registration order = dispatch order

`ink/hooks/use-input.ts:62-68` — listener is registered once on mount; its slot in the EventEmitter's listener array is stable. `BaseTextInput` registers its `useInput` first because **child effects fire before parent effects**, so the leaf widget gets first-look at every keystroke. `useTextInput.ts:320-331`:

> "Skip when a keybinding context (e.g. Autocomplete) owns escape. useKeybindings can't shield us via stopImmediatePropagation — BaseTextInput's useInput registers first (child effects fire before parent effects), so this handler has already run by the time the keybinding's handler stops propagation."

In Rust this maps to event-routing order: children before parents. The leaf input must handle keys _before_ any global keybinding tries to.

### 3.8 The eleven Big Ideas (claudy)

1. **One reader on stdin.** Two readers drop bytes.
2. **Focus is derived, not stored.** Compute it from a small expression over global state and local conditions.
3. **Refs for state read in the same tick.** Ref = truth, state = render trigger.
4. **Keybindings are declarative; contextual keys stay imperative.** Verbs vs intrinsic UX.
5. **Mount order = registration order = dispatch order.** Children before parents.
6. **Pure resolution.** `resolve(input, key, contexts, bindings, pending)` is a pure function returning a discriminated union.
7. **Specificity ranking is explicit.** `[...activeContexts, callerContext, 'Global']` then dedup. Most-specific wins; within a context, last-binding-wins.
8. **One declared cursor per frame.** Inactive widgets don't clobber active ones (node-identity check).
9. **Overlays register themselves via RAII.** Mount/unmount = add/remove from a `Set<id>`.
10. **Two reducers in CustomSelect.** Cursor and selection are independent.
11. **Text-editing quirks are local handlers, not keybindings.** Double-Esc clear, DEL coalescing, SSH-coalesced Enter need the raw byte stream.

---

## 4. Synthesis: design rules for our TUI

We adopt **codex's structural rules** + **claudy's interaction discipline** + **claudy's keybinding system**. Concretely:

| Codex provides                                                                                                                           | Claudy provides                                                                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Layered architecture, terminal owner, two event types, four-source loop, FrameRequester, view-stack focus model, Renderable trait family | Focus-as-derived-state, RAII overlay registry, declarative keybindings with scoped contexts + chords + last-binding-wins, two-reducer selection, refs for race-prone state, mount-order rule |

The runner TUI is small enough that we can be more aggressive than codex (no app-server, no streaming markdown, no per-thread state, no transcript). We get a **simpler** version of the same architecture.

### 4.1 The eight rules we will enforce

1. **The terminal is owned by `Tui`.** Nothing else touches `stdout`, raw mode, bracketed paste, focus events, or frame scheduling. App + below ask `Tui` to do things; never reach past it.
2. **Two event types: `TuiEvent` (physical) and `AppEvent` (logical).** Single `select!` over them plus IPC.
3. **Render is a pure function of widget state.** `Renderable::render(&self, area, buf)`. Never mutates, never schedules, never sends events.
4. **Mutation goes through `AppEvent`.** Widgets hold an `AppEventSender`, never a back-reference to App. The dispatcher is one flat match.
5. **Modality lives in a single `view_stack: Vec<Box<dyn View>>`.** No focus pointer; the top of the stack is the input target. Auto-pop on `is_complete()`. (We start with one stack only — no full-screen overlays for the runner. Adding `Option<Overlay>` later costs nothing.)
6. **Focus _within_ the base view is derived.** The base view is "tabs + per-tab pane state." Each pane gets `is_focused: bool` at render time, computed from the active-tab's pane enum. No pane stores its own `is_focused`.
7. **Keybindings are declarative + scoped.** A small registry: `(KeyEvent, Context)` → `Action`. Resolution is a pure function. Most-specific context wins; last-binding-wins.
8. **One stdin reader; `KeyEventKind::Release` filtered at every layer.** Bracketed paste enabled; `Event::Paste` mapped through. Paste-burst as fallback for terminals that don't support bracketed paste.

---

## 5. Target architecture for the runner TUI

### 5.1 Module layout

```
runner/src/tui/
├── mod.rs                   // pub fn run; thin entry point
├── tui.rs                   // Tui struct: terminal owner; init/restore/with_restored
│                            //   exposes draw(), event_stream(), frame_requester()
├── tui/
│   ├── event_stream.rs      // crossterm Event → TuiEvent { Key, Paste, Resize, Draw }
│   ├── frame_requester.rs   // FrameRequester (handle) + FrameScheduler (actor)
│   └── frame_rate_limiter.rs// 120fps clamp
│
├── app.rs                   // App struct + run loop (the four-source select!)
├── app/
│   ├── event_dispatch.rs    // single flat match on AppEvent
│   ├── input.rs             // top-level key routing (overlay → view-stack → base)
│   └── ipc_listener.rs      // background task: IPC events → AppEvent::Ipc*
│
├── event.rs                 // AppEvent enum
├── event_sender.rs          // AppEventSender (clone, convenience methods)
│
├── render/
│   └── renderable.rs        // trait Renderable + ColumnRenderable, FlexRenderable
│
├── view/
│   ├── mod.rs               // trait View: Renderable + handle_key + is_complete + …
│   └── completion.rs        // ViewCompletion::{Accepted, Cancelled}
│
├── input/
│   ├── paste_burst.rs       // pure state machine
│   └── keymap/
│       ├── mod.rs           // KeymapRegistry, Action enum, Context enum
│       ├── resolver.rs      // pure fn resolve(key, contexts, registry) → Resolution
│       └── default_bindings.rs
│
├── widgets/
│   ├── scroll_state.rs      // ScrollState { selected: Option<usize>, scroll_top }
│   ├── selectable_list.rs   // SelectableList<Id>: identity-stable selection
│   ├── textarea.rs          // single-line text editor (owns text + cursor)
│   ├── picker.rs            // refactored to use SelectableList
│   └── focus_border.rs      // helper: Block::default().border_style(if focused …)
│
├── views/
│   ├── base.rs              // BaseView: tabs + per-tab pane focus
│   ├── general.rs
│   ├── runner_status.rs
│   ├── runs.rs
│   ├── approvals.rs
│   └── modals/              // each modal = struct impl View
│       ├── confirm.rs
│       ├── add_runner.rs
│       ├── register.rs
│       └── config_edit.rs
│
└── ipc_client.rs            // unchanged
```

Mapping to codex layers:

| Layer | codex                      | runner                                           |
| ----- | -------------------------- | ------------------------------------------------ |
| L1    | main.rs, lib::run_main     | runner cli → `tui::run`                          |
| L2    | tui.rs + tui/              | `tui.rs` + `tui/`                                |
| L3    | app.rs, app/, app_event    | `app.rs`, `app/`, `event.rs`, `event_sender.rs`  |
| L4    | chatwidget.rs (per-thread) | **(omitted — runner has no per-thread state)**   |
| L5    | bottom_pane/               | merged into `views/base.rs` + `view::View` trait |
| L6    | composer + popups          | `widgets/textarea.rs` + `widgets/picker.rs`      |
| L7    | render/                    | `render/`                                        |

We omit L4 entirely (no per-thread / streaming / transcript) and merge L5 into the base view (the runner has 4 tabs, not a chat surface). This keeps the runner TUI ~1500 LOC instead of codex's 60k+ while preserving the architectural shape.

### 5.2 Core types

```rust
// tui/event_stream.rs
pub enum TuiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize(u16, u16),
    Draw,
}
```

```rust
// event.rs
pub enum AppEvent {
    // Lifecycle
    Quit,
    Tick,                                  // ticker fires (~500ms)

    // IPC results (posted by background ipc_listener)
    IpcStatusUpdated(StatusSnapshot),
    IpcApprovalsUpdated(Vec<Approval>),
    IpcRunsUpdated(Vec<Run>),
    IpcError(String),

    // Modal intent (any view can request these)
    PushView(Box<dyn View>),
    PopView,                               // explicit pop request (Esc fallback)

    // Side-effects requested by views (kept in app/event_dispatch)
    SubmitAddRunner(AddRunnerForm),
    SubmitRegister(RegisterForm),
    PersistConfig { field: ConfigField, value: String },
    Bell,
    // …
}
```

```rust
// event_sender.rs
#[derive(Clone)]
pub struct AppEventSender {
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl AppEventSender {
    pub fn send(&self, ev: AppEvent) { let _ = self.tx.send(ev); }

    // convenience wrappers (codex pattern)
    pub fn quit(&self)                                    { self.send(AppEvent::Quit); }
    pub fn push_view(&self, v: Box<dyn View>)             { self.send(AppEvent::PushView(v)); }
    pub fn submit_register(&self, form: RegisterForm)     { self.send(AppEvent::SubmitRegister(form)); }
    // …
}
```

```rust
// render/renderable.rs (codex shape)
pub trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16 { 1 }
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> { None }
}
```

```rust
// view/mod.rs
pub enum KeyHandled { Consumed, NotConsumed }

pub trait View: Renderable {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut ViewCtx<'_>) -> KeyHandled;
    fn handle_paste(&mut self, _text: String, _ctx: &mut ViewCtx<'_>) -> KeyHandled {
        KeyHandled::NotConsumed
    }
    fn is_complete(&self) -> bool { false }
    fn completion(&self) -> Option<ViewCompletion> { None }
    fn dismiss_after_child_accept(&self) -> bool { false }
    fn is_modal(&self) -> bool { true }              // true: input does not fall through
    fn on_ctrl_c(&mut self) -> Cancellation { Cancellation::NotHandled }
    fn prefer_esc_to_handle_key(&self) -> bool { false }
}

pub struct ViewCtx<'a> {
    pub tx: &'a AppEventSender,
    pub keymap: &'a KeymapRegistry,
    pub paths: &'a Paths,
}
```

Note the `KeyHandled` return convention is borrowed from claudy (`useKeybinding.ts:113-121`): a view can decline to consume so the key falls through to the underlying handler. Most modals will return `Consumed` for plain `Char`; only popups that overlay a still-active text input (not a use case in the runner today) would return `NotConsumed`.

```rust
// widgets/scroll_state.rs (port verbatim from codex)
pub struct ScrollState {
    pub selected: Option<usize>,
    pub scroll_top: usize,
}

impl ScrollState {
    pub fn clamp(&mut self, len: usize)                           { … }
    pub fn move_up_wrap(&mut self, len: usize)                    { … }
    pub fn move_down_wrap(&mut self, len: usize)                  { … }
    pub fn ensure_visible(&mut self, len: usize, rows: usize)     { … }
}
```

```rust
// widgets/selectable_list.rs
pub struct SelectableList<Id: Eq + Clone> {
    state: ScrollState,
    selected_id: Option<Id>,        // identity-stable selection (claudy validatedFocusedValue)
}

impl<Id: Eq + Clone> SelectableList<Id> {
    pub fn reconcile(&mut self, items: &[Id]) {
        // If selected_id still exists in items, snap selected index to it.
        // Else clamp to len; pick first if items non-empty.
    }
    pub fn move_up(&mut self, items: &[Id]);
    pub fn move_down(&mut self, items: &[Id]);
    pub fn selected_id(&self) -> Option<&Id>;
    pub fn render<R: Fn(&Id, bool) -> Line>(
        &self, area: Rect, buf: &mut Buffer,
        items: &[Id], focused: bool, fmt: R,
    );
}
```

```rust
// input/keymap/mod.rs
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    Global,
    Tabs,                                  // tab-switch + global hotkeys
    List,                                  // j/k/Enter/d/etc. on a list
    TextInput,                             // active text field
    ConfirmDialog,                         // y/n
    Picker,                                // search-as-you-type list
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Quit, NextTab, PrevTab, GoToTab(u8),
    ListUp, ListDown, ListAccept, ListCancel, ListPageUp, ListPageDown,
    SubmitForm, FieldNext, FieldPrev,
    OpenAddRunner, RemoveSelectedRunner,
    // …
}
```

```rust
// input/keymap/resolver.rs (port shape from claudy/keybindings/resolver.ts)
pub enum Resolution {
    Match(Action),
    Unbound,                               // explicitly null-bound (suppresses default)
    None,
    ChordStarted(Vec<KeyEvent>),
    ChordCancelled,
}

pub fn resolve(
    key: KeyEvent,
    active_contexts: &[Context],           // most-specific first; Global last
    registry: &KeymapRegistry,
    pending: Option<&[KeyEvent]>,
) -> Resolution { … }
```

The resolver is a **pure function**. Active-contexts list is computed at the call site by `app::input` based on the current view-stack and base view: `[…view_specific_contexts, Tabs, Global]`. **Most-specific wins; last-binding-wins.**

### 5.3 The event loop

```rust
// app.rs (target shape, ~120 lines)
pub async fn run(paths: Paths, initial_tab: Tab) -> Result<()> {
    let mut tui = Tui::init()?;                      // raw + bracketed paste + focus events + panic hook
    let _restore = TerminalRestoreGuard;              // RAII cleanup

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);

    let mut app = App::new(paths, initial_tab, sender.clone(), tui.frame_requester().clone());

    // No long-lived IPC listener: the App ticker is the sole cadence owner (§5.7).
    // Each Tick spawns one-shot tasks per concern, gated by in-flight flags.

    let mut tui_events = tui.event_stream();
    tokio::pin!(tui_events);
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    tui.frame_requester().schedule_frame();

    loop {
        select! {
            Some(ev) = rx.recv() =>
                app.dispatch_app_event(&mut tui, ev).await?,

            Some(ev) = tui_events.next() =>
                app.handle_tui_event(&mut tui, ev).await?,

            _ = ticker.tick() =>
                app.event_tx.send(AppEvent::Tick),
        }
        if app.quit { break; }
    }

    Ok(())
}
```

Three sources, not four (we have no per-thread channel). `dispatch_app_event` is a flat match (codex style); `handle_tui_event` routes to the input pipeline (§5.5). Draws are pulled out as `TuiEvent::Draw` from the stream — just like codex. The 500ms ticker is the only thing that cares about wall-clock time; everything else is event-driven.

### 5.4 The view stack

```rust
pub struct App {
    base: BaseView,                                // tabs + per-tab state
    view_stack: Vec<Box<dyn View>>,                // modals
    keymap: KeymapRegistry,
    event_tx: AppEventSender,
    frame: FrameRequester,
    paths: Paths,
    quit: bool,
    // cached IPC snapshots (refreshed via AppEvent::Ipc*Updated)
    status: Option<StatusSnapshot>,
    approvals: Vec<Approval>,
    runs: Vec<Run>,
}

impl App {
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        // 1. Render base view always.
        self.base.render(area, frame.buffer_mut());
        // 2. Render each modal in stack order on top.
        for view in &self.view_stack {
            view.render(area, frame.buffer_mut());
        }
        // 3. Cursor: top-of-stack first, else base.
        let cursor = self.view_stack.last()
            .and_then(|v| v.cursor_pos(area))
            .or_else(|| self.base.cursor_pos(area));
        if let Some((x, y)) = cursor { frame.set_cursor_position((x, y)); }
    }

    fn dispatch_key(&mut self, key: KeyEvent) {
        // 1. Top-of-stack gets first look.
        if let Some(view) = self.view_stack.last_mut() {
            let mut ctx = ViewCtx { tx: &self.event_tx, keymap: &self.keymap, paths: &self.paths };
            let handled = view.handle_key(key, &mut ctx);
            if view.is_complete() {
                self.pop_with_completion(view.completion());
            }
            if matches!(handled, KeyHandled::Consumed) || view.is_modal() {
                return;
            }
        }
        // 2. Base view (with global keymap context).
        self.base.handle_key(key, …);
    }

    fn pop_with_completion(&mut self, c: Option<ViewCompletion>) {
        self.view_stack.pop();
        match c {
            Some(ViewCompletion::Accepted) => {
                while self.view_stack.last().map_or(false, |v| v.dismiss_after_child_accept()) {
                    self.view_stack.pop();
                }
            }
            _ => {}
        }
    }
}
```

This is **codex's pattern, half-sized**: one stack, no overlay layer. Adding a full-screen overlay later means adding `overlay: Option<Overlay>` to `App` and branching on it in `render` / `dispatch_key` first.

### 5.5 The input pipeline

```
crossterm Event
  → tui::event_stream::map (drop Mouse/FocusLost; FocusGained→Draw; Paste passthrough)
  → KeyEventKind::Press|Repeat filter (single source-of-truth here)
  → app::handle_tui_event
      ├── TuiEvent::Resize → resize_reflow + schedule_frame
      ├── TuiEvent::Draw   → tui.draw(|f| app.render(f))
      ├── TuiEvent::Paste  → app.dispatch_paste(text)
      └── TuiEvent::Key    → app.dispatch_key(key)
```

**Routing order for `dispatch_key`** — strictly five layers, evaluated in sequence; each can return `Consumed` to stop or `NotConsumed` to fall through:

1. **View-stack top** (transient modal, if any). If the top view returns `Consumed` _or_ `is_modal()` is true, stop — global hotkeys are not consulted under a modal.
2. **Focused child within the base view** (the textarea or list that owns the cursor for the active tab+pane). This is the claudy "leaf first" rule: a focused `TextArea` sees `Char('1')` before any keymap can resolve it as `Tabs::GoToTab(0)`.
3. **Pane-level bindings** — keymap resolution under `Context::List` / `Context::Picker` / `Context::ConfirmDialog`, depending on which pane is focused.
4. **Tab-level bindings** — `Context::Tabs` (Tab / Shift+Tab / digit jumps).
5. **Global bindings** — `Context::Global` (Ctrl+C, `?`, etc.).

`app::dispatch_key` enforces this order. Each base view exposes `fn focused_child(&mut self) -> Option<&mut dyn KeyTarget>` and `fn active_contexts(&self) -> &[Context]`. The keymap resolver is only called _after_ the focused child returns `NotConsumed`. **A keymap layer never sees a key the focused child wanted to consume** — this is what claudy enforces with mount-order in `useInput` (`use-input.ts:62-68`), and what we enforce here with explicit routing.

**Three invariants** that fix Bug 3 by construction:

1. Only `widgets/textarea.rs` inserts characters. Forms own a `TextArea` per field; they never have their own `Char(c)` arm. The textarea also redundantly filters `KeyEventKind::Release` (defense in depth).
2. When a textarea is the focused child, layer 2 returns `Consumed` for any printable key. Layers 3–5 never resolve digit/letter keys against `Tabs`/`Global` while editing. The base view's `active_contexts()` _also_ excludes `Tabs`/`Global` while a text input is focused — defense in depth: even if layer 2 returned `NotConsumed` for some key, the keymap can't accidentally tab-switch a digit.
3. **Paste-burst flushes before any modified key.** Non-ASCII chars bypass the buffer and insert directly (claudy `paste_burst.rs` rule).

### 5.6 Focus model

Same as claudy:

- The `BaseView` knows its active tab and per-tab pane state. There is **no `is_focused` field on any widget.**
- At render time, `BaseView::render` computes `is_focused` for each child and passes it as a parameter to `child.render(area, buf, focused: bool)`. The widget renders its border / highlight from this boolean.
- A view-stack push doesn't change the base focus state at all. The base just stops getting key events; visual focus is unchanged underneath the modal.

For the runner today the focus expression is much simpler than claudy's:

```rust
// Approvals tab, two cards
let pending_focused = matches!(state.approvals_pane, Pane::Pending);
let detail_focused  = matches!(state.approvals_pane, Pane::Detail);

self.pending_list.render(left, buf, pending_focused, …);
self.detail_view.render(right, buf, detail_focused, …);
```

The `Pane` enum per tab is the entire focus state. No collisions, no aliasing.

### 5.7 Background work — IPC

**One cadence owner: the App ticker.** Background tasks are _result-producing_, not cadence-owning. Codex's pattern (`app/background_requests.rs`) is fire-and-forget: a tokio task is spawned to run _one_ request, posts an `AppEvent::*Loaded` variant when it completes, and exits. There are no peer cadence loops.

For the runner this becomes:

```rust
// app/event_dispatch.rs — the dispatcher's Tick handler
fn handle_tick(&mut self) {
    if !self.ipc.status_in_flight {
        self.ipc.status_in_flight = true;
        let tx = self.event_tx.clone();
        let paths = self.paths.clone();
        tokio::spawn(async move {
            let result = ipc_client::status(&paths).await;
            tx.send(AppEvent::IpcStatusUpdated(result));
        });
    }
    // Approvals: same pattern, always-on (the bell behavior depends on it).
    // Runs: same pattern, but only spawn when self.tab == Tab::Runs.
}

// On AppEvent::IpcStatusUpdated(result):
fn handle_status_updated(&mut self, result: Result<StatusSnapshot, IpcError>) {
    self.ipc.status_in_flight = false;
    match result {
        Ok(s)  => self.status = Some(s),
        Err(e) => self.ipc_error = Some(e.to_string()),
    }
    self.frame.schedule_frame();
}
```

Properties:

- **Single cadence owner** — the App's 500ms ticker (§5.3). All polling is gated through it, so ordering is deterministic: a Tick fires, the dispatcher decides what to refresh, spawns one task per concern, the task posts results back through the bus. Two ticks cannot race.
- **In-flight gate per concern** (`status_in_flight`, `approvals_in_flight`, `runs_in_flight`) prevents pile-up if a request takes longer than the tick interval — the next Tick simply skips that concern.
- **Per-tab gating** for `runs` — only spawn while `Tab::Runs` is the active tab (mirrors today's behavior at `app.rs:551-561`).
- **IPC never blocks the input loop** — every IPC call runs on a spawned task, never inside the `select!` arm that holds `&mut state`.
- **Backpressure is implicit** — if the bus has no subscribers (App is exiting), `tx.send` fails and the task exits naturally. No CancellationToken needed.

There is no long-lived `spawn_ipc_listener`. The frame scheduler remains the only forever-living background task.

### 5.8 The keymap — a small registry

```rust
// input/keymap/default_bindings.rs
pub fn defaults() -> KeymapRegistry {
    KeymapRegistry::from_blocks(vec![
        block(Context::Global, &[
            (key("ctrl+c"),     Action::Quit),
            (key("?"),          Action::OpenHelp),
        ]),
        block(Context::Tabs, &[
            (key("tab"),        Action::NextTab),
            (key("shift+tab"),  Action::PrevTab),
            (key("1"),          Action::GoToTab(0)),
            (key("2"),          Action::GoToTab(1)),
            (key("3"),          Action::GoToTab(2)),
            (key("4"),          Action::GoToTab(3)),
        ]),
        block(Context::List, &[
            (key("j"),          Action::ListDown),
            (key("k"),          Action::ListUp),
            (key("down"),       Action::ListDown),
            (key("up"),         Action::ListUp),
            (key("enter"),      Action::ListAccept),
            (key("esc"),        Action::ListCancel),
        ]),
        // …
    ])
}
```

Active contexts at the call site (claudy rule): `[…view_specific, Tabs, Global]`. When a textarea is focused, the active contexts become `[TextInput]` only — `Tabs` is _not_ in the list, so digit keys cannot be resolved as tab jumps. **This is the architectural fix for Bug 3.**

### 5.9 The frame requester

Verbatim port of `codex-rs/tui/src/tui/frame_requester.rs`. ~150 LOC after trimming to our needs (no draw broadcast for multiple subscribers — we have one subscriber). The 120fps clamp stays.

### 5.10 Testing surface

Three layers, mirroring codex:

1. **Pure-fn unit tests** for `ScrollState`, `paste_burst`, `keymap::resolve`, `SelectableList::reconcile`. These are the highest-leverage tests: they replicate codex's `bottom_pane/scroll_state.rs` test shape.
2. **`Buffer` snapshot tests** for each `View` impl. Each view exposes `render(area, buf)`; tests render into a `Buffer`, dump the cells, snapshot with `insta`. Just like `codex-rs/tui/src/bottom_pane/snapshots/`.
3. **Loop-driven integration tests** with `ratatui::backend::TestBackend` and a synthetic `TuiEvent` stream. The dispatcher and view-stack are exercised end-to-end. Following codex (`app/tests.rs`), tests don't run `App::run`'s `select!` — they call `app.dispatch_app_event(…)` and `app.handle_tui_event(…)` directly with a hand-built `(AppEventSender, mpsc::Receiver)` pair.

### 5.11 Validation adjustments from reference audit

After re-reading the current runner code and both reference codebases, four adjustments are needed so this plan stays aligned with the architectures we are borrowing from:

1. **Keep first-run registration in `GeneralView`; do not move it into `view_stack`.**
   - In the current runner, registration is the base content of the General tab when config is missing, not a transient popup.
   - In codex, the stack is for transient picker / approval / confirmation views; in claudy, overlays are likewise mounted only for temporary modal state.
   - Turning registration into a stacked modal would fight the doc's own rule that base-view focus is derived from tab state.

2. **Keep inline config editing as widget-local state, not a modal `View`.**
   - The edit buffer is an inline field editor. That matches codex's composer / textarea split and claudy's `BaseTextInput` pattern much better than a popup layer.
   - The right target is: each editable pane owns a `TextArea`-like widget or field-editor state; Esc / Enter stay local to that pane unless an actual modal is open.

3. **Use one refresh cadence owner, not two.**
   - The draft currently defines both `AppEvent::Tick` and a forever-looping `ipc_listener` that sleeps 500ms and polls.
   - Codex centralizes ordering through one bus, but it does not duplicate the same cadence in two different subsystems.
   - Recommendation: keep a single ticker in `App::run` that triggers poll requests, with background tasks only returning results. Also keep `runs` polling gated to the Runs tab; approvals can stay always-on if the bell behavior is retained.

4. **Make child-first key routing explicit inside each base tab.**
   - Claudy relies on leaf-first input handling: focused text inputs and focused selects see keys before container-level bindings.
   - Our base-view plan already routes stack-top before base, but the same rule must also apply inside `BaseView`: focused child widget first, then pane-level keymap, then tab/global keymap.
   - Without this, we risk rebuilding the same "text field vs global hotkey" class of bugs one layer lower.

---

# Part II — Migration plan

## 6. Audit of the current TUI (recap)

Symptoms:

1. **Focus is invisible / drifts.** No focus manager; each tab invents its own focus model. Borders only highlight on the Runners tab, and only between two cards.
2. **List items can't be selected reliably.** `ListState` is throwaway — built fresh inside each `render` (`runs.rs:28-32`, `approvals.rs:28-34`, `runner_status.rs:159-161`) — so scroll offset never persists. `state.selected` resets to 0 on every tab change and refresh. On Runners, `state.runner_picker_idx` (drives highlight) and `state.runners_list_idx` (drives `[d]` action) are aliased and unsynchronized.
3. **Text input swallows characters.** `register_form` matches digits and `h`/`l`/Left/Right at `app.rs:776-786` and **falls through to the global tab switcher** — typing `1`/`2`/`3`/`4`/`h`/`l` into Cloud URL or hostname switches tabs. `Event::Paste` is never handled; bracketed paste isn't enabled.

Architectural smells driving them:

- One monolithic `handle_event` (`app.rs:564-1080`, ~520 lines), modal precedence encoded as declaration order.
- Single flat `AppState` (~30 fields), no per-view structs.
- IPC `await`ed inline — input loop blocks while config.toml is read.
- Render every loop iteration — no coalescing; `print!("\x07")` mid-render corrupts the buffer (`app.rs:521`).

The target architecture (Part I) eliminates each of these by construction.

## 7. Phases

Each phase compiles and passes `cargo test` and `cargo clippy -- -D warnings` independently. Each is a separate PR (≤ ~600 LOC diff target).

### Phase 0 — scaffolding (no behavior change)

- Add `tui.rs` (Tui struct), `tui/event_stream.rs`, `tui/frame_requester.rs`, `tui/frame_rate_limiter.rs` — but the existing app.rs continues to use its current event loop. The new `Tui` is built but only `frame_requester()` and `event_stream()` are wired in.
- Add `event.rs` (AppEvent enum), `event_sender.rs` — empty bus, not yet routed.
- Add `widgets/scroll_state.rs` with full unit tests (port codex's tests).
- Add `view/mod.rs` (View trait + ViewCompletion).
- Add `render/renderable.rs` (Renderable trait + ColumnRenderable, FlexRenderable).
- Add `input/keymap/{mod,resolver,default_bindings}.rs` with unit tests for resolver.
- **Acceptance:** binary unchanged; new modules covered by tests; `cargo build`+`clippy` clean.

### Phase 1 — event loop & input pipeline

- Replace `app::loop_ui` with the three-source `select!` (Part I §5.3).
- Wrap crossterm `EventStream` into `TuiEvent` (centralized `KeyEventKind::Release` filter + `Event::Paste` mapping).
- Enable bracketed paste; handle `TuiEvent::Paste`.
- Move IPC off the input path. Use one cadence owner only: the app ticker triggers poll requests; background tasks return `AppEvent::Ipc*Updated`.
- Replace per-loop draw with `FrameRequester` + `TuiEvent::Draw`-driven render.
- Migrate `app::handle_event`'s register-form arm into structured `GeneralView` state first; do **not** move registration into `view_stack`.
- **Acceptance:** typing `1`/`2`/`3`/`4`/`h`/`l` into Cloud URL/hostname/token fields no longer switches tabs; bracketed-paste of a multi-line URL inserts intact; `cargo test` green.

### Phase 2 — view stack + remaining modals

- Convert `confirm_exit`, `confirm_stop`, `remove_runner_confirm`, `help`, and `add_runner_form` to `View` impls.
- Keep config-field editing inline inside the owning tab/view state; do not push a `config_edit` modal for single-field edits.
- Delete the corresponding modal early-returns from the old `handle_event`.
- Add `app::pop_with_completion` and `dismiss_after_child_accept` chaining.
- **Acceptance:** every modal opens/closes; Esc pops top modal universally; global hotkeys disabled while a modal is open (`view_stack.last().is_modal() == true`).

### Phase 3 — selectable list + per-view state

- Convert each tab to a struct in `views/`: `GeneralView`, `RunnerStatusView`, `RunsView`, `ApprovalsView`. Each owns its own `SelectableList<Id>`.
- Delete `state.selected`, `state.runner_picker_idx`, `state.runners_list_idx`, `state.tab_general_field`, `state.runner_tab_focus`. They become per-view fields.
- `[d]` removal on Runners reads from the list's selection (single source of truth).
- **Acceptance:** scroll offset persists across redraws (Approvals with 50+ items); selection survives the 500ms tick; `[d]` removes the highlighted runner.

### Phase 4 — paste burst + textarea

- Vendor `paste_burst.rs` from codex with attribution comment (Apache-2.0; both projects).
- Implement `widgets/textarea.rs` (single-line; we don't need wrap / kill-buffer / placeholder yet — about 250 LOC).
- Wire textarea + paste-burst into all form fields. Delete remaining ad-hoc `Char(c)` handling.
- Paste-blocks-Enter rule (claudy `BaseTextInput.tsx:59-66`).
- **Acceptance:** rapid pasted text into hostname inserts as one chunk; no mid-paste tab switches; Enter inside paste-burst does not submit.

### Phase 5 — declarative keymap (cleanup)

- Move all hotkeys to `default_bindings.rs`. Each view declares its active contexts via a small method (`fn active_contexts(&self) -> &[Context]`).
- `app::input` builds `[…view_contexts, Tabs, Global]` and calls `keymap::resolve`.
- Action handlers replace inline match arms.
- **Acceptance:** the audit's three concerns become impossible: focus is owned by per-view state; selection is owned by per-view `SelectableList`; text input uses the keymap with `TextInput`-only context, so digit keys cannot escape.

Optional follow-ups, not part of this refactor:

- User-overridable bindings (`~/.config/runner/keybindings.toml`) — claudy's `loadUserBindings.ts` shape.
- `Option<Overlay>` for full-screen views (not needed today; trivial to add later).
- Multi-line textarea (when we add a free-text approvals comment field, etc.).

## 8. Bug-by-bug mapping (verification)

### Bug 1 — focus indicator

- Per-tab `Pane` enum is the entire focus state.
- `is_focused: bool` is passed to each widget at render time.
- View-stack pushes leave base focus untouched.

### Bug 2 — list selection

- `SelectableList<Id>` owns its `ScrollState` across frames.
- `reconcile(items)` after refresh restores selection by Id; clamps if id is gone.
- One field per visible list — no aliasing.
- `[d]` reads `list.selected_id()` — same source as the highlight.

### Bug 3 — swallowed chars

- `KeyEventKind::Press|Repeat` filtered at the stream boundary (defense-in-depth: also at textarea).
- `Event::Paste` mapped to `TuiEvent::Paste`; routes straight to focused textarea.
- Active-contexts list excludes `Tabs` / `Global` digit/letter bindings when a textarea is focused.
- Paste-burst handles legacy paste streams; flushes before modified keys.
- Paste-blocks-Enter while burst non-empty.

## 9. Testing strategy

| Test type        | What it covers                                                               | Tool                                 |
| ---------------- | ---------------------------------------------------------------------------- | ------------------------------------ |
| Pure-fn unit     | `ScrollState`, `paste_burst`, `keymap::resolve`, `SelectableList::reconcile` | Plain `#[test]`                      |
| Buffer snapshot  | Each `View::render` output                                                   | `ratatui::buffer::Buffer` + `insta`  |
| Loop integration | `app.dispatch_*` end-to-end                                                  | `TestBackend` + synthetic `TuiEvent` |
| Manual smoke     | Real terminal: paste, modal flow, tab switches mid-edit                      | `runner/README.md` checklist         |

Manual smoke checklist (kept short — same as before):

- Type `1234567890` into hostname — all 10 chars inserted; no tab switches.
- Tab between cards on every tab — visual focus indicator follows.
- Open Approvals, scroll past visible window, wait for tick — selection retained.
- Paste a multi-line URL — inserted as one chunk; Enter inside paste does not submit.
- Modal open → Esc closes; global keys disabled while open.

## 10. Risks

| Risk                                                              | Mitigation                                                                                                                  |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Phased migration leaves the codebase in mixed-architecture states | Each phase is a working binary; fall-back paths preserved until the new path is proven                                      |
| Phase 1 changes too much at once (bus + IPC + paste handling)     | Phase 1 is the largest; can be split into 1a (event loop + paste) and 1b (IPC bus migration) if review feedback asks for it |
| `SelectableList<Id>` Id collisions                                | Use stable identifiers from the data model (runner id, approval id, run id); document the contract                          |
| Background IPC tasks leak on shutdown                             | `Drop` of `AppEventSender` closes the channel; the listener observes `Send` failure and exits                               |
| Bracketed paste varies by terminal                                | Paste-burst is the fallback; both engaged together                                                                          |
| Vendoring `paste_burst.rs` from codex                             | Both Apache-2.0; include attribution comment; keep updates one-way (theirs → ours)                                          |

## 11. Open questions

1. **Vendor or rewrite `paste_burst.rs` and `scroll_state.rs`?** Recommendation: vendor, with attribution; trim to single-line use.
2. **Should the General tab also use `SelectableList<FieldId>` or stay with a `FieldCursor` enum?** Recommendation: SelectableList for consistency in Phase 3.
3. **Do we want user-overridable keybindings shipping with this refactor?** Recommendation: no — defer to a follow-up. The architecture supports it; the surface area is bigger than needed for the bug fixes.
4. **Where do IPC errors render?** Recommendation: a status-bar widget at the bottom that subscribes to `AppEvent::IpcError` (so errors don't pile up in the bus).

---

## 12. Appendix — citation index

### codex-rs (architecture)

- Lifecycle: `lib.rs:678` (`run_main`), `tui.rs:272-296` (`init` + panic hook), `tui.rs:313-368` (Tui struct + `new`), `app.rs:659-1109` (`App::run`), `lib.rs:1604-1634` (`TerminalRestoreGuard`).
- Pause primitive: `tui.rs:412-444` (`with_restored`); rationale `tui/event_stream.rs:9-18`.
- AppEvent bus: `app_event.rs:1-9` (rationale), `app_event.rs:120-805` (variants), `app_event.rs:184-185` (channel-of-channels rationale).
- AppEventSender: `app_event_sender.rs:18-115`; session-log integration `:31-37`.
- Dispatcher: `app/event_dispatch.rs:12-1966` (giant flat match).
- Four-source select: `app.rs:1020-1068`.
- Round-robin starvation prevention: `tui/event_stream.rs:265-291`.
- Renderable trait: `render/renderable.rs:13-19`; combinators `:141-307`.
- BottomPaneView trait: `bottom_pane/bottom_pane_view.rs:18-133`.
- view_stack push/pop: `bottom_pane/mod.rs:199, 447-473, 531-575`.
- Esc routing: `bottom_pane/bottom_pane_view.rs:65`, `list_selection_view.rs:938-940`.
- Composer layered handlers: `bottom_pane/chat_composer.rs:1548, 1582, 3095`.
- TextArea: `bottom_pane/textarea.rs:1-11, 90-99, 331-454`.
- Release filter (4 layers): `bottom_pane/mod.rs:531-534`, `chat_composer.rs:1553-1555`, `chat_composer.rs:3098-3100`, `textarea.rs:331-336`.
- Paste burst: `bottom_pane/paste_burst.rs` (entire file); flush-before-modified `chat_composer.rs:3092-3094`.
- Frame scheduler: `tui/frame_requester.rs:30-128`; rate limit `tui/frame_rate_limiter.rs:13`.
- Background work tied by ownership: `app.rs:572` (per-thread JoinHandles), `frame_requester.rs:42` (FrameScheduler spawn).
- HistoryCell trait: `history_cell.rs:113-205`; transcript_cells `app.rs:525`.

### claudy (interaction discipline)

- Single stdin reader: `ink/components/App.tsx:332`; early-input handoff `:222-228`; bug rationale `hooks/usePasteHandler.ts:208-212`.
- Keybindings:
  - Defaults: `keybindings/defaultBindings.ts:32-340`.
  - Resolver (pure): `keybindings/resolver.ts:32-244`; chord algorithm `:166-244`.
  - Conflict rule: `keybindings/useKeybinding.ts:53-60` (specificity); `resolver.ts:42-50, 223-229` (last-binding-wins).
  - `Consumed | NotConsumed`: `useKeybinding.ts:113-121`.
  - Reserved: `reservedShortcuts.ts:16-33`.
- Focus as derived: `components/PromptInput/PromptInput.tsx:2199`.
- Overlay registry RAII: `context/overlayContext.tsx:38-104`; non-modal allowlist `:21`.
- Hardware cursor parking: `ink/hooks/use-declared-cursor.ts:25-73`.
- CustomSelect:
  - Two reducers: `use-select-state.ts:127-158`, `use-select-navigation.ts:74-180`.
  - Identity-stable selection: `use-select-navigation.ts:592-602`.
  - Wrap-out callbacks: `use-select-input.ts:117-135`.
  - Input-row escape hatch: `use-select-input.ts:115-148`.
- Refs for race-prone state:
  - Paste pending: `hooks/usePasteHandler.ts:48-53`.
  - Chord pending: `keybindings/KeybindingProviderSetup.tsx:135-187`.
- Mount-order rule: `ink/hooks/use-input.ts:62-68`; `hooks/useTextInput.ts:320-331`.
- Text-editing quirks:
  - DEL coalescing: `useTextInput.ts:442-465`.
  - SSH-coalesced Enter: `useTextInput.ts:485-499`.
  - Paste-blocks-Enter: `components/BaseTextInput.tsx:59-66`.
  - Double-Esc clear (intentionally not a keybinding): `useTextInput.ts:122-153`.
- Listener gating by `isActive`: `BaseTextInput.tsx:88-90`, `ink/hooks/use-input.ts:50-81`.

### Current runner TUI (audit)

- Event loop: `runner/src/tui/app.rs:382-404`.
- Monolithic `handle_event`: `runner/src/tui/app.rs:564-1080`.
- Register-form fall-through (Bug 3 epicenter): `runner/src/tui/app.rs:766-836`, digit/`h`/`l`/Left/Right at `:776-786`.
- `state.selected` resets: `app.rs:881-898, 963, 973, 1533`.
- Selection alias collision: `runner_picker_idx` in `runner_status.rs:96, 107, 160` vs `runners_list_idx` in `app.rs:1000, 1328`.
- Throwaway `ListState`: `runs.rs:28-32`, `approvals.rs:28-34`, `runner_status.rs:159-161`.
- Bell mid-render corrupts buffer: `app.rs:521`.
- IPC blocks input loop: `app.rs:499-562`.
- No bracketed paste: `app.rs:368` (raw mode without `EnableBracketedPaste`); `app.rs:565` (`Event::Key` only matched).
