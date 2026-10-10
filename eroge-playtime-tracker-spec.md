# Eroge Playtime Tracker — Technical Specification

> Working title: `ErogePlaytimeTracker`  
> Target: Windows 10 / 11 x64  
> Status: Initial implementation specification  
> Primary use case: Record play time for visual novels / eroge per title, with ErogameScape metadata.

## 1. Goal

Create a Windows desktop application that:

- Tracks play sessions for registered games.
- Counts play time as **session running time minus background time** by default, with an app-wide setting to include background time.
- Records one logical `PlaySession` per game launch.
- Always preserves background periods inside that launch as `BackgroundInterval` records, regardless of the calculation setting.
- Lets the user review and edit recorded sessions later.
- Retrieves game metadata such as title, brand, release date, and thumbnail from ErogameScape (批評空間).
- Lets the user filter games by brand and sort by values such as total play time and last played time.
- Runs as a normal Windows application and can stay resident in the system tray.
- Does not require the user to manually install .NET or another application runtime.

This is a **local-first desktop application**. A server/backend service is not required.

---

## 2. Chosen technology stack

### Desktop framework

- **Tauri 2**

### Backend / native logic

- **Rust**
- Windows API access via the `windows` crate.
- HTTP client: `reqwest`.
- HTML parsing: choose a maintained Rust HTML parser such as `scraper`.
- SQLite access: **`rusqlite`**.
- Serialization: `serde` / `serde_json`.

### Frontend

- **Svelte**
- **TypeScript**
- Vite-based Tauri frontend.

The frontend is responsible for presentation and user interaction only.  
Tracking, database access, metadata retrieval, and Windows integration belong in Rust.

### Database

- **SQLite**
- One local database file.
- Use migrations from the beginning.

---

## 3. High-level architecture

```text
Svelte / TypeScript UI
        |
        | Tauri commands / events
        v
Rust application layer
├── tracking
│   ├── process detection
│   ├── foreground-window detection
│   └── session state machine
├── database
│   ├── migrations
│   └── repositories
├── metadata
│   └── ErogameScape provider
├── thumbnails
└── settings
        |
        v
SQLite + local thumbnail cache
```

The tracking code should not depend directly on Svelte or UI state.  
Keep the core tracking logic independently testable.

Suggested Rust layout:

```text
src-tauri/src/
├── main.rs
├── commands/
│   ├── games.rs
│   ├── sessions.rs
│   └── settings.rs
├── database/
│   ├── mod.rs
│   ├── migrations.rs
│   ├── game_repository.rs
│   └── session_repository.rs
├── metadata/
│   ├── mod.rs
│   └── erogamescape.rs
├── models/
│   ├── game.rs
│   ├── brand.rs
│   ├── executable.rs
│   ├── play_session.rs
│   └── focus_interval.rs
├── tracking/
│   ├── mod.rs
│   ├── foreground.rs
│   ├── process.rs
│   └── session_tracker.rs
└── settings/
    └── mod.rs
```

Exact module names may change if there is a clear architectural reason.

---

## 4. Core domain model

### 4.1 Game

Represents one visual novel / eroge.

Suggested fields:

```text
Game
- id
- erogamescape_id       nullable
- title
- brand_id              nullable
- release_date          nullable
- thumbnail_path        nullable
- source_url            nullable
- created_at
- updated_at
```

### 4.2 Brand

```text
Brand
- id
- erogamescape_id       nullable
- name
```

### 4.3 GameExecutable

A game may have multiple executables.

```text
GameExecutable
- id
- game_id
- path
- file_name
- created_at
```

Important:

- Do **not** assume Game : Executable is 1:1.
- Launcher / engine / secondary executable configurations must be possible.
- Executable matching should prefer normalized full paths when available.

Example:

```text
Game
└── Executables
    ├── launcher.exe
    └── game.exe
```

### 4.4 PlaySession

One logical record per game launch that presents a visible window.

```text
PlaySession
- id
- game_id
- launched_at
- exited_at             nullable while running
- created_at
- updated_at
```

`launched_at` is the timestamp at which a visible top-level window owned by an associated game process is first detected. `exited_at` is the last window-loss timestamp once process exit confirms shutdown. Initialization before the first visible window is not recorded. A process that exits without presenting a visible window creates no session. This start rule applies to new recordings; existing history is preserved.

Do not store total play duration as the source of truth if it can be calculated.

### 4.5 BackgroundInterval

Represents a period during a `PlaySession` in which the running game was not the foreground game.

```text
BackgroundInterval
- id
- play_session_id
- started_at
- ended_at              nullable while active
- created_at
- updated_at
```

With the default `exclude_background_time = true`, the play time of a session is:

```text
(PlaySession.exited_at - PlaySession.launched_at)
- SUM(BackgroundInterval.ended_at - BackgroundInterval.started_at)
```

The total play time of a game is the sum across all of its sessions.

With `exclude_background_time = false`, use the entire session duration without subtracting background intervals. Apply the saved setting to existing history as well as active sessions, substituting the current time for an open end. Never stop recording background intervals or persist a calculated duration when this setting changes.

This model intentionally preserves:

- total process/session time;
- recorded background time;
- derived play time.

Example:

```text
Game window shown:  20:00
Foreground:          20:00 - 20:30
Browser foreground:  20:30 - 20:40
Foreground:          20:40 - 21:20
Discord foreground:  21:20 - 21:30
Foreground:          21:30 - 22:00
Game exited:         22:00

Session duration:    2h 00m
Background time:     0h 20m
Playtime:            1h 40m
```

---

## 5. Tracking behavior

### 5.1 Primary rule

The default play-time definition is:

> Start with the time for which the game session existed, then exclude periods in which the game was in the background.

A game can remain running in the background. Always record that period, including when `exclude_background_time = false`; the setting changes calculation only, not tracking state or interval recording.

### 5.2 Foreground-window detection

Use Windows APIs through the Rust `windows` crate.

Primary APIs / concepts:

- `SetWinEventHook`
- `EVENT_SYSTEM_FOREGROUND`
- `GetForegroundWindow`
- `GetWindowThreadProcessId`

Expected flow:

```text
Foreground window changed
        |
        v
Get owning PID
        |
        v
Resolve executable
        |
        v
Match executable against registered GameExecutable rows
        |
        +--> registered game -> end its BackgroundInterval
        |
        +--> anything else   -> begin BackgroundInterval for running games
```

### 5.3 Event-driven + reconciliation

Use foreground-change events as the normal mechanism.

Also implement a low-frequency reconciliation check using `GetForegroundWindow()` so the tracker can recover if an event is missed.

A reasonable initial reconciliation interval is approximately **2–5 seconds**.  
Keep the value configurable internally.

Do not use high-frequency polling when it is unnecessary.

### 5.4 Process / launch tracking

The application must know when a registered game starts, presents a visible window, and exits so that one `PlaySession` corresponds to one launch that presents a visible window.

Implementation may use Windows process APIs, event subscriptions, or a conservative polling mechanism, but should satisfy these behaviors:

1. Registered game starts. Track it as Starting without creating a session.
2. On first detection of an associated visible top-level window, create one `PlaySession` with that timestamp as `launched_at`, whether or not the window is foreground. If every associated process exits before this happens, stop tracking without creating history.
3. Foreground transitions create/end `BackgroundInterval` records.
4. Registered game exits.
5. End any open `BackgroundInterval`.
6. Set `PlaySession.exited_at`.

Monitor associated process handles for exit and trigger reconciliation immediately when a PID exits. The periodic reconciliation remains as a fallback. When a game owns multiple associated processes, an individual PID exit must not end the session while another associated process remains alive.

Derive tracking state from process existence, visible top-level windows owned by associated PIDs, and foreground ownership. A process with no visible game window is Starting (before the first window) or WindowTransition/ShuttingDown (after a window existed), not Background. Record Background only while a visible game window exists and none of the game's windows owns the foreground. Close a session at the last window-loss time after process exit confirms shutdown.

### 5.5 Multiple executables for one game

Multiple registered executables may map to the same game.

Avoid generating duplicate simultaneous sessions when a launcher starts the actual game executable.

Define at most one logical session per game while at least one associated process is alive. A session starts only after the first associated visible window is detected.

State should therefore be tracked by **game**, not merely by PID.

Suggested interpretation:

```text
0 matching processes alive -> game not running
1+ matching processes alive, no window observed yet -> Starting, no session
first associated visible window -> start the session
later window transitions while a process remains alive -> keep the same session
```

Transition from 0 to 1+ starts process tracking; the first visible window starts recording. Transition from 1+ to 0 ends the session, if one was created. A windowless bootstrap may exit and relaunch through any launcher without creating an extra session. Do not combine independent launches based on elapsed time.

### 5.6 Multiple registered games running

The design should tolerate multiple registered games running at the same time.

- Each running game may have its own `PlaySession`.
- Every running game other than the foreground game accumulates a `BackgroundInterval`.
- Switching foreground directly from Game A to Game B starts A's interval and ends B's interval.

---

## 6. Editing behavior

The user must be able to inspect and edit recorded data.

### Session list

For a game, show at minimum:

- launch/start datetime;
- exit/end datetime;
- derived play time;
- recorded background time;
- process/session duration.

### Session editing

Allow editing:

- `launched_at`;
- `exited_at`.

### Background interval editing

Because background intervals are the source of truth for background time regardless of calculation settings, provide a detailed session view that allows:

- adding a background interval;
- editing start/end of a background interval;
- deleting an incorrect background interval.

Validation:

- `ended_at >= started_at`;
- background intervals should remain inside the parent session's start/end range when the session has both bounds;
- reject or normalize overlapping background intervals inside the same session;
- changing session bounds must not silently produce invalid background intervals.

### Manual session creation

Allow a user to add a session manually.

For a simple manual entry, it is acceptable to create:

- one `PlaySession`;
- no `BackgroundInterval` (the manually entered range is all play time).

### Legacy compatibility

Keep the original `focus_intervals` table for rollback compatibility. Existing focus data is migrated by storing its complement inside each closed session as `background_intervals`. New tracking and manual edits keep `focus_intervals` updated as a compatibility mirror, while current-version calculations use session durations with optional background subtraction. The compatibility mirror and migration's session-minus-background validation must not depend on the calculation setting.

---

## 7. ErogameScape / 批評空間 metadata

### Purpose

Retrieve and populate game metadata such as:

- title;
- brand;
- release date;
- thumbnail / package image when available;
- ErogameScape game ID;
- source URL.

### Provider abstraction

Do not couple the application directly to one scraper implementation.

Define a provider abstraction conceptually similar to:

```rust
trait GameMetadataProvider {
    async fn fetch_game(&self, id: /* ... */) -> Result<GameMetadata, MetadataError>;
    async fn search(&self, query: &str) -> Result<Vec<GameSearchResult>, MetadataError>;
}
```

Exact Rust async-trait mechanics may differ.

Implement:

```text
ErogameScapeProvider
```

behind this abstraction.

This is important because the HTML structure may change in the future and other providers could be added later.

### Input methods

MVP should support at least one reliable registration method:

- ErogameScape game URL; or
- ErogameScape game ID.

Title search can be implemented if reliable enough, but URL/ID registration is sufficient for the first working milestone.

### Scraping behavior

- Use `reqwest`.
- Parse HTML using a maintained parser.
- Keep CSS selectors / parsing details isolated inside the ErogameScape provider.
- Use a sensible User-Agent.
- Avoid aggressive request rates.
- Do not repeatedly fetch unchanged metadata on every application launch.

### Thumbnail behavior

Download the thumbnail once and cache it locally.

Suggested data directory:

```text
%LOCALAPPDATA%\ErogePlaytimeTracker\
├── app.db
├── thumbnails\
└── logs\
```

Do not depend on ErogameScape being reachable merely to render the existing game library.

---

## 8. UI requirements

The UI does not need elaborate visual effects. Prefer a clean desktop-library layout.

### 8.1 Main game library

Each game entry should show at least:

- thumbnail;
- title;
- brand;
- total play time;
- last played datetime.

### 8.2 Filter

Support filtering by:

- brand.

Also provide a title/text search if inexpensive to implement.

### 8.3 Sorting

Support at least:

- title;
- brand;
- release date;
- registration date;
- total play time;
- last played datetime;
- play-session count.

Ascending/descending switching should be possible where appropriate.

### 8.4 Game detail

Show:

- thumbnail;
- title;
- brand;
- release date;
- registered executable paths;
- total play time;
- total launched/running time if useful;
- last played datetime;
- session history.

Actions:

- add/remove executable;
- edit metadata where reasonable;
- refresh ErogameScape metadata;
- add/edit/delete session records.

### 8.5 Add game

Initial flow:

1. Enter ErogameScape URL or ID.
2. Fetch metadata.
3. Show preview.
4. Register game.
5. Add one or more executable paths.

Also permit a manually-created game when metadata retrieval fails.

### 8.6 System tray

The app should support resident operation.

Tray behavior:

- opening the main window;
- showing basic tracking state;
- exiting the application cleanly.

Closing the main window may minimize/hide to tray rather than terminate the tracker.  
Make the behavior explicit in settings if needed.

### 8.7 Auto-start

Support optional launch at Windows sign-in using an appropriate Tauri 2 plugin or equivalent native implementation.

Default may be off for the first build.

### 8.8 Play-time calculation setting

Provide an app-wide checkbox labeled `バックグラウンド時間をプレイ時間から除外する`, enabled by default. Save it as `AppSettings.exclude_background_time` in the existing settings JSON. Missing fields in older JSON default to `true`; backup export/import preserves the setting.

Saving applies to all games and existing history without a restart. Use neutral labels such as `バックグラウンド時間` and `バックグラウンド区間` for recorded data. Explain whether the current setting subtracts or includes these periods, and that recording continues in both modes. Deletion confirmations must not promise that deleting an interval always increases play time.

---

## 9. Tauri command boundary

Do not expose raw SQL or low-level tracking internals to the frontend.

Commands should be task-oriented, for example:

```text
list_games
get_game
create_game
update_game
delete_game

add_game_executable
remove_game_executable

list_sessions
create_manual_session
update_session
delete_session

list_background_intervals
create_background_interval
update_background_interval
delete_background_interval

fetch_erogamescape_metadata
refresh_game_metadata

get_settings
update_settings
```

Tracking status changes can be pushed to the frontend using Tauri events where appropriate.

Expose tracking status per game with a phase of `starting`, `foreground`, `background`, or `window_transition`. The header may show multiple phase chips at once, ordered foreground, background, starting, then window transition. Show a title for a single game and aggregate multiple games in the same phase by count, retaining their titles in tooltip/detail text.

Starting games remain in both event and polling status responses with `session_id: null` until a session is created. They still block update installation/relaunch and backup operations. Screenshot and OCR operations require a persisted session and the game's current foreground window.

---

## 10. Database notes

Use SQLite migrations.

Recommended initial tables:

```text
brands
games
game_executables
play_sessions
background_intervals
focus_intervals            legacy compatibility mirror
settings
```

Potential indexes:

```text
game_executables(path)
play_sessions(game_id, launched_at)
background_intervals(play_session_id, started_at)
focus_intervals(play_session_id, started_at)
games(brand_id)
```

Store timestamps consistently.

Recommended choice:

- UTC in the database;
- convert to local time in the UI.

If another timestamp representation is chosen, keep it consistent throughout the application.

Foreign keys should be enabled.

Deletion behavior should be intentional, e.g. deleting a game may cascade to its executables/sessions after explicit confirmation.

---

## 11. Derived statistics

Do not duplicate statistics in the database unless profiling proves it necessary.

Derive:

### Total play time

When background exclusion is enabled (the default):

```text
SUM(all PlaySession durations for game)
- SUM(all BackgroundInterval durations for game)
```

When disabled, use only the sum of session durations. Use the same saved setting for game lists/details and sorting, session history, timestamp cumulative/difference values, and both app-wide and per-game statistics. Count background-only days/sessions in statistics when their time is included. The last-played definition below is independent of this setting.

### Last played time

Prefer:

```text
The end of the most recent foreground portion, derived from the session end and any trailing BackgroundInterval.
```

with a reasonable fallback for an active session if needed.

### Session count

```text
COUNT(PlaySession)
```

### Total running time

```text
SUM(PlaySession.exited_at - PlaySession.launched_at)
```

---

## 12. Runtime state

The tracker should maintain an in-memory state representing:

```text
registered executable paths
running games
running process IDs per game
active foreground game
optional open PlaySession IDs per running game (none before the first visible window)
open BackgroundInterval IDs by session
open legacy FocusInterval ID
```

Database writes should happen at meaningful state transitions rather than on every polling tick.

Examples of write points:

- a running game presents its first visible window -> start its session;
- foreground switches to registered game;
- foreground leaves registered game;
- game transitions from running -> not-running.

---

## 13. Reliability requirements

Tracking correctness is more important than extremely small code size.

Handle gracefully:

- process disappears between enumeration and inspection;
- executable path cannot be read due to permissions;
- foreground HWND becomes invalid;
- game exits while focused;
- app exits while a session is active;
- transient database failures;
- ErogameScape unavailable or HTML parsing fails;
- thumbnail download failure.

Failures in metadata retrieval must not stop local play-time tracking.

Log useful errors locally without logging unnecessarily sensitive data.

### Crash / unclean-exit recovery

For the first implementation, at minimum detect open sessions / intervals left from a previous unclean app shutdown.

Do not silently count time from the crash until the next app start.

A conservative recovery strategy is acceptable, such as closing orphaned records at a persisted last-seen timestamp or marking them for user review.

---

## 14. Privacy and network behavior

The core tracker is local.

Network access should occur only for metadata/thumbnail retrieval or other explicitly networked features.

Screenshot OCR runs locally. When the user chooses Google search, open a Google search URL containing the full recognized text in the default browser. Only this explicit search action sends recognized text to Google; screenshot images are not uploaded.

A separately configured OCR search hotkey starts region selection over the foreground tracked game's client area. Keep the game foreground and show a transparent, non-activating selection outline over its live image. Selection clicks and cancellation keys must not operate the game. Releasing a valid selection removes the outline, captures the current frame in memory without creating a screenshot file or history record, runs local OCR, and opens Google search for the full nonempty result. Escape, right-click, focus loss, or display/client-area changes cancel selection without searching or restoring focus. The game is not paused and normal foreground/background-time accounting continues. Report failures in the app without activating a modal dialog. Exclusive-fullscreen overlay/capture compatibility remains game-dependent.

Do not upload play history.

Do not require an account.

---

## 15. Distribution

Target:

```text
Windows 10 / 11 x64
```

Primary deliverable:

- Windows `.exe` installer generated through Tauri.

The user should not need to manually install .NET, Node.js, Rust, or development tooling.

### WebView2

Tauri uses Microsoft WebView2 on Windows.

Packaging should ensure a normal end user does not have to manually resolve this dependency.

Preferred approach:

- use Tauri's supported Windows installer behavior for WebView2 installation/bootstrap; or
- bundle a fixed WebView2 runtime if a fully self-contained distribution is explicitly chosen later.

A literal portable one-file executable is **not a hard MVP requirement** if it would make WebView2 distribution unnecessarily complex.  
A single installer `.exe` with no manual prerequisite installation is acceptable.

---

## 16. Development principles

- Keep native Windows / process / foreground logic in Rust.
- Keep persistence behind repository/service boundaries.
- Keep ErogameScape parsing isolated behind a provider abstraction.
- Keep Svelte focused on UI.
- Prefer simple, explicit code over framework-heavy architecture.
- Avoid premature optimization.
- Write tests for tracking state transitions and time aggregation.
- Add migrations instead of mutating schema ad hoc.
- Do not store redundant duration values as authoritative state.

---

## 17. Suggested implementation order

### Phase 1 — project skeleton

- Initialize Tauri 2 + Svelte + TypeScript.
- Add Rust module layout.
- Add SQLite database and migration infrastructure.
- Establish application data directory.

### Phase 2 — local library CRUD

- Game CRUD.
- Brand CRUD/internal handling.
- Multiple executable registrations per game.
- Basic library UI.
- Game detail UI.

### Phase 3 — tracking MVP

- Detect registered processes.
- Start/end one `PlaySession` per logical game launch.
- Detect foreground game.
- Record `BackgroundInterval`s and maintain the legacy focus mirror.
- Display live/current tracking state.
- Aggregate total play time as session time minus background time.

### Phase 4 — history

- Session list.
- Session detail.
- Manual session creation.
- Edit/delete session.
- Edit/add/delete focus intervals.
- Validation for overlaps/ranges.

### Phase 5 — ErogameScape

- Provider abstraction.
- URL/ID metadata retrieval.
- Brand mapping.
- Thumbnail caching.
- Metadata refresh.
- Graceful parsing/network errors.

### Phase 6 — library features

- Brand filter.
- Sorting.
- Title search.
- Last played / total playtime / session count statistics.

### Phase 7 — resident app behavior

- System tray.
- Hide/show window behavior.
- Optional auto-start.
- Clean shutdown of active records.
- Recovery of orphaned records.

### Phase 8 — packaging and quality

- Windows x64 packaging.
- WebView2 installer behavior.
- Tests.
- Logging.
- README/build instructions.

---

## 18. MVP acceptance criteria

The first usable release is complete when all of the following work:

1. The application runs on Windows 10/11 x64.
2. A game can be registered manually.
3. Multiple executable paths can be attached to one game.
4. The first associated visible window of a registered game creates one `PlaySession`. A windowless startup stays tracked without creating history.
5. By default, visible-game background periods do not accumulate play time.
6. Alt-Tabbing away records background time; whether that time contributes to play time follows the saved setting.
7. Returning to the game ends the background interval inside the same launch session.
8. Exiting the game closes the session.
9. Session history can be viewed.
10. Session and background-interval data can be edited.
11. Total play time is derived correctly in both modes, including past sessions, open sessions, timestamps, sorting, and statistics. Toggling the setting never discards background records.
12. Games can be filtered by brand.
13. Games can be sorted by total play time and last played time.
14. Game metadata can be populated from ErogameScape by URL or ID.
15. Thumbnails are cached locally.
16. The app can remain resident in the system tray.
17. Metadata/network failure does not break play-time tracking.
18. End users do not need to manually install .NET, Rust, Node.js, or development tools.

---

## 19. Explicit non-goals for the initial version

Unless necessary for the architecture, do not prioritize:

- cloud synchronization;
- user accounts;
- mobile support;
- macOS/Linux support;
- social features;
- achievements;
- automatic game discovery across the whole disk;
- Steam integration;
- complex analytics dashboards;
- plugin systems;
- multiple metadata providers beyond the abstraction needed to support them later.

---

## 20. Important design decisions to preserve

These decisions are intentional and should not be casually changed during implementation:

1. **Rust + Tauri 2 + Svelte + TypeScript** is the selected stack.
2. **SQLite + rusqlite** is the selected local persistence approach.
3. Session time minus background time is the default play-time metric. Users may include background time without changing how it is recorded.
4. A launch is represented by `PlaySession`.
5. Background periods inside a launch are represented by `BackgroundInterval`.
6. A game can have multiple registered executables.
7. Session state is tracked per game, not per PID.
8. Durations are derived from timestamps rather than treated as independent authoritative values.
9. ErogameScape integration is isolated behind a provider abstraction.
10. Local tracking must continue to work when ErogameScape/network access fails.
11. A Windows installer `.exe` with automatic dependency handling is acceptable for MVP.
