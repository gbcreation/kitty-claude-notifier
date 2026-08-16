# kitty-claude-notifier

Reactive Kitty tab indicators for Claude Code, in Rust. See `README.md`
for the "why." In short, a background daemon reads a tab's actual
rendered screen content to detect when a permission prompt has been
resolved, since Claude Code fires no hook for that specific transition
(it's a menu selection, not a submitted message). It also prepends a
small colored icon onto a tab's existing title rather than replacing it
outright.

## Build, Install, Test

```bash
# Build
cargo build --release

# Install (copies the binary to ~/.config/kitty-claude-notifier/bin/,
# writes a default config.toml if none exists, appends hooks into
# ~/.claude/settings.json without disturbing any other entries already there)
./target/release/kitty-claude-notifier install

# Uninstall (removes only this tool's hook entries + copied binary;
# preserves config.toml)
./target/release/kitty-claude-notifier uninstall

# Test suite (must be green before pushing)
cargo test --all-targets

# Lint + format (CI runs the same three checks)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Architecture

```
src/
├── main.rs                  # entry point: clap parse, dispatch
├── lib.rs                   # module declarations
├── cli.rs                   # clap CLI: hook / daemon / install / uninstall / test
├── state.rs                 # State enum (locked model) + default icon glyph/color
├── config.rs                # config.toml load; IconSpec/ColorSpec + per-state overrides
├── icon.rs                   # builds/strips the colored icon prefix on a title
├── paths.rs                 # ~/.config/kitty-claude-notifier/{config.toml,daemon.sock,daemon.log,bin/}
├── markers.rs                # permission-prompt text matching (tail-of-screen, case-insensitive)
├── sound.rs                  # opt-in sound playback on permission/waiting/done (background thread)
├── install.rs                # install/uninstall; pure merge_hooks/remove_hooks + file I/O wrapper
├── hook/
│   ├── mod.rs                # hook subcommand: parse stdin, resolve target, send to daemon
│   ├── payload.rs            # HookPayload: subset of Claude Code's hook JSON we use
│   └── event.rs               # (event, Notification-matcher) -> State mapping
├── ipc/
│   ├── protocol.rs            # HookMessage / MessageKind: the wire format
│   └── connect.rs             # connect_or_spawn_daemon: fast-path connect, else spawn + retry
├── kitty/
│   ├── mod.rs                 # KittyClient trait + TabInfo + apply()/clear() (title fetch/build/strip + color)
│   ├── process_client.rs      # real impl: shells out to `kitten @ ... --to unix:$KITTY_LISTEN_ON`
│   ├── target.rs              # WindowTarget resolution (KITTY_WINDOW_ID, else parent PID)
│   └── fake.rs                 # (test-only) in-memory KittyClient for deterministic unit tests
└── daemon/
    ├── mod.rs                  # daemon subcommand: logging init, flock, bind, serve
    ├── server.rs                # UnixListener accept loop; reloads config.toml fresh per message
    ├── session.rs               # in-memory SessionTable (no persistence)
    ├── transitions.rs           # the state machine: applies hook messages
    ├── idle_timer.rs            # per-session cancellable Done/Waiting -> Idle timer
    └── resume_watch.rs          # per-session cancellable screen-scrape resume detector

tests/
├── kitty_ipc.rs              # ProcessKittyClient against a mock kitten binary
├── daemon_lifecycle.rs        # the real compiled daemon, isolated $HOME, real socket
└── support/mock_kitten.sh     # the mock kitten script both integration tests can use
```

### Layer rules

- `hook/` resolves everything that only makes sense in the hook process's
  own environment (which Kitty window, which state) and hands off a
  message. It never touches Kitty directly.
- `daemon/` owns all Kitty IPC and all session state. Nothing outside
  `daemon/` should call into `kitty::` except `hook`'s message-building
  path and `main.rs`'s `test` subcommand (which deliberately bypasses the
  daemon entirely, per its own doc comment).
- `kitty::KittyClient` is a trait specifically so `daemon/`'s logic can be
  unit-tested against `kitty::fake::FakeKittyClient` without a real Kitty
  instance. Keep it that way; don't let `daemon/` modules construct a
  `ProcessKittyClient` directly inside logic that should be testable.
- `icon.rs` is pure string logic (no I/O, no `KittyClient` dependency) so
  the build/strip round-trip can be unit-tested directly. `kitty::apply()`/
  `clear()` are the only callers that combine it with actual Kitty IPC.
- `install.rs` keeps its JSON transform (`merge_hooks`/`remove_hooks`)
  separate from file I/O specifically so it's unit-testable without
  touching a real `settings.json` or mutating `$HOME`.

### Data flow

```
Claude Code fires hook event
  → kitty-claude-notifier hook --event <name> [--matcher <m>] --stdin
    → resolves WindowTarget from env (KITTY_WINDOW_ID, else parent PID)
    → maps (event, matcher) -> State via hook::event::resolve_state
    → sends one NDJSON message to the daemon's Unix socket,
      spawning it (detached) first if nothing answers
    → process exits

daemon (long-running, tokio)
  → accepts the connection, reads the message
  → server::handle_connection() reloads config.toml fresh from disk for
    *this* message (falls back to Config::default() on a parse error,
    e.g. mid-edit, never fatal); no daemon restart needed to pick up
    an edit; see server.rs's load_config()
  → daemon::transitions::apply():
      - kitty::apply(): base title = config's `text` override if set for
        this state, otherwise the tab's live title (get_tab_info, via
        `kitten @ ls`, which also returns focus state, see below);
        build_title() strips any icon this tool previously applied and
        prepends the new one; set_tab_color() sets active_bg/inactive_bg;
        never fatal, failures are logged. Returns the tab's focus
        state if get_tab_info happened to be called (None if the `text`
        override skipped it)
      - if this is a genuine transition (the new state differs from the
        one already recorded for this session), retires any timers the
        superseded state armed, then arms idle_timer (Done/Waiting)
        and/or resume_watch (Permission only) for the new state. A
        repeat of the same state leaves existing timers untouched
        instead (see Known limitations)
      - if config.sound_enabled and this is a genuine transition, and
        (the tab isn't currently focused OR config.sound_play_when_focused),
        plays a sound in a background thread for Permission/Waiting
        (request) or Done (done); see transitions::sound_for_transition.
        Reuses kitty::apply()'s focus result if it fetched one; falls
        back to its own get_tab_info call otherwise (only when a `text`
        override was configured for this state, and only when
        sound_play_when_focused is false, since there's no need to know
        the actual focus state if it won't be checked)
  → idle_timer fires exact-timing -> Idle after config.idle_timeout_secs
  → resume_watch (Permission only) polls get-text every
    config.resume_poll_interval_ms, flips -> Working after 2 consecutive
    polls without any configured permission_markers present
  → Cleanup (session-end): kitty::clear() strips the icon back off
    (restoring the tab's natural title) and clears the tab color
```

## State model (LOCKED)

Do not add, remove, or rename states without explicit approval. States
without any wired hook trigger are intentionally omitted from this model.

| State | Entry trigger | Exit |
|-------|--------------|------|
| `working` | `UserPromptSubmit`; Notification `elicitation_response` | superseded by the next hook message |
| `permission` | Notification `permission_prompt` | `resume_watch`: markers cleared for 2 consecutive polls |
| `waiting` | Notification `idle_prompt`/`elicitation_dialog` | `idle_timer`: exact `idle_timeout_secs` after entry, or superseded by the next hook message (typically `UserPromptSubmit` once you reply) |
| `done` | `Stop` | `idle_timer`: exact `idle_timeout_secs` after entry |
| `idle` | daemon-internal (`idle_timer` firing) | superseded by the next hook message |
| `error` | `StopFailure` | superseded by the next hook message |

`session-end` is not a state; it's a cleanup signal (`hook::event::resolve_state`
returns `None` for it, handled as `MessageKind::Cleanup`: resets the tab,
removes the session and aborts its timers).

`PostToolUseFailure` is deliberately **not** registered as a hook at all
(not just unmapped): a single tool call failing is not surfaced as
`Error`. Claude routinely recovers from a failed tool call within the
same turn without any input from the user, so flagging every one was
noise, and (before `Error` had any exit condition) could leave a tab
stuck on "error" long after Claude had already moved on. `StopFailure`
(the whole turn ending in failure) is kept; nothing "tries something
else" after that.

## Known limitations

- **`resume_watch` (screen-scraping) is armed only for `Permission`, not
  `Waiting`.** A `Permission` dialog is answered via a menu selection
  (arrow keys + enter), which fires no hook at all on resolution, so
  screen-scraping against `permission_markers` is the only signal
  available. `Waiting` is different: a real reply is typed and
  submitted, which already fires `UserPromptSubmit`, already mapped to
  `working`, so it resolves correctly through hooks alone. An earlier
  version of this daemon armed `resume_watch` for `Waiting` too, reusing
  the same `permission_markers` list, but that's wrong: the thing on
  screen while `Waiting` is just Claude Code's ordinary input box, and
  its content (an optional AI-suggested reply) varies every time, so
  there's no fixed wording to watch for disappearing. Confirmed live: it
  was self-clearing back to `working` within about one second of
  entering `Waiting`, incorrectly, since the marker list never matched
  that box's arbitrary content. Do not re-add `resume_watch` for
  `Waiting` without a fundamentally different (non-marker-based) way to
  detect resolution.
- **A repeated `SetState` into the same state a session is already in
  leaves its timers untouched**, rather than aborting and re-arming them.
  This matters because Claude Code appears to re-fire `idle_prompt`
  periodically while `Waiting` (observed roughly every 60 seconds in a
  live session's `daemon.log`). If every repeat reset `idle_timer`'s
  300-second countdown from zero, a session could stay stuck on
  `waiting` forever, as long as the nudges kept arriving faster than the
  timeout. See `transitions::apply`'s `old_state == Some(state)` early
  return.
- **Split panes sharing one Kitty tab**: `set_tab_title`/`set_tab_color`
  operate on the whole tab, not an individual pane. If two Claude Code
  sessions run in two panes of the same tab (Kitty's split layout), they
  share one title and one background color slot, so whichever session's
  hook fires most recently overwrites the other's icon. There's no way
  to show two independent indicators for sessions sharing a tab; Kitty
  has no per-pane title in its tab bar to target instead.

## Icon prepending

- Each state has a glyph + color (`config.icon_for(state)` → `Icon`),
  prepended onto the tab's title rather than replacing it. Verified
  live that Kitty's tab bar renders raw ANSI truecolor SGR escapes
  (`\x1b[38;2;r;g;bm...\x1b[39m`) embedded directly in a title string set
  via `kitten @ set-tab-title`. This isn't documented Kitty behavior;
  don't assume it without re-verifying if Kitty's tab-bar rendering ever
  changes.
- **Icons should be plain Unicode symbols or Nerd Font glyphs, not
  emoji, if you want `color` to have any visible effect.** This isn't a
  hard technical restriction: `build_title`/`strip_icon_prefix` treat
  `glyph` as an opaque string, so an emoji round-trips and renders in
  Kitty's tab bar just fine. But emoji carry their own embedded color
  glyph (COLR/emoji font tables) and silently ignore SGR foreground
  color, so a configured `color` is simply never applied to one.
  Confirmed by a failed live test (a colored ANSI wrapper around an
  emoji rendered in the emoji's own default color, not the requested
  one) before switching the built-in defaults to plain characters
  (`●`, `▲`, `◐`, `✓`, `○`, `✕`).
- Built-in default icon colors are white for every state except `Idle`.
  **Do not** default an icon's color to match its state's tab
  background color (`default_color()`); if they're equal, the icon is
  invisible against its own background, a real bug caught only by live
  testing, not unit tests.
- `Icon.text`, if set in config, is used as the title's base *instead of*
  fetching the tab's live title. `kitty::apply()` skips the
  `get_tab_info` call entirely in that case, not just the result (which
  also means it can't report focus state in that case; see Sound
  notifications below for how the sound branch compensates).
- `icon::build_title()`/`strip_icon_prefix()` must round-trip correctly
  even when the base title is empty. A prior bug had `build_title`
  `.trim_end()` away the space `strip_icon_prefix` depended on to find
  its own reset marker, silently breaking icon-stripping specifically
  for empty-base titles. Any change to the wrapper format needs a
  round-trip test (see `icon.rs`'s `round_trips_correctly_when_base_title_is_empty`).

## Sound notifications

- Off by default (`sound_enabled = false`), a bigger behavior change
  than a visual tweak, so it's opt-in.
- `transitions::sound_for_transition(sound_enabled, old_state, new_state)`
  is pure decision logic (no I/O): `Permission`/`Waiting` → `Sound::Request`,
  `Done` → `Sound::Done`, everything else → `None`. Also returns `None`
  when `new_state == old_state`, since a repeated/duplicate hook firing
  for a state the session is already in must not replay the sound. This
  function does *not* know about focus; that's a separate guard at the
  call site (below), kept out of this pure function since it needs I/O.
- **Focus suppression**: a tab you're already looking at doesn't need an
  audio nudge too. `kitty::apply()` already fetches `TabInfo.is_focused`
  while fetching the live title (unless a `text` override skips that
  entirely). The sound branch in `transitions::apply()` reuses that
  result if present, and falls back to its own `client.get_tab_info()`
  call only when it's `None` (i.e. only in the `text`-override case, and
  only when a sound-eligible transition is actually about to fire, never
  wasted on a disabled/no-op transition). On a failed fallback fetch, it
  fails open (assumes unfocused, plays the sound) rather than silently
  swallowing a real notification.
- `config.sound_play_when_focused` (off by default) opts back into
  playing regardless of focus. When true, the focus check (and the
  fallback `get_tab_info()` fetch behind it) is skipped entirely via
  short-circuit (`config.sound_play_when_focused || !is_focused`), not
  just ignored after fetching; there's no reason to spend an IPC call
  finding out a focus state the config says not to care about.
- `sound::play()` always spawns a `std::thread` and returns immediately;
  never let audio playback block the daemon's message loop.
- `sound::play()` has a hard `if cfg!(test) { return; }` guard at the top.
  This must stay a compile-time check, not a config/runtime toggle. It's
  the only thing guaranteeing no test run ever spawns a real audio
  process, regardless of what `sound_enabled` is set to in a test's config.
- Built-in sounds (`assets/sounds/request.mp3`, `assets/sounds/done.mp3`)
  are embedded via `include_bytes!`, so there's no runtime file
  dependency. They're copied byte-for-byte from `herdrdev/herdr` (Apache
  License 2.0); see `assets/sounds/NOTICE.md` for attribution and the
  verified upstream git blob SHA-1s. Don't replace them without updating
  that file.
- A custom `[sounds]` path is tried first; on any failure (missing file,
  player exits nonzero) it falls back to the built-in default rather than
  playing nothing.
- No audio library dependency: shells out to whichever system player is
  found first (`afplay` on macOS; `paplay`/`pw-play`/`ffplay`/`mpg123`/`mpv`
  on Linux, tried in that order). A player failing to spawn falls through
  to the next one; a player that spawns but exits nonzero is a final
  failure (logged via `tracing::warn!`, not retried against the next
  player), the same fallback boundary herdr's own `sound.rs` draws.
- **Real bug hit once during manual testing, not a code defect**: enabling
  this by hand-editing the live config with `echo 'sound_enabled = true'
  >> config.toml` appended the key *after* the last `[table]` header,
  silently nesting it inside that table instead of the document root.
  This is the same TOML-ordering pitfall `config/default.toml`'s header
  comment and `config.rs`'s `shipped_default_config_parses` regression
  test already guard against. No parse error, no log line; the only
  symptom was silence, since serde just ignores an unrecognized key on a
  table it doesn't `deny_unknown_fields` on.

## Code style

- Rust 2024 edition, `cargo fmt` defaults, `cargo clippy -D warnings` clean.
- Prefer let-chains (`if let Some(x) = a && let Some(y) = b`) over nested
  `if let` per clippy's `collapsible_if`.
- Every `KittyClient` call from `daemon/` goes through `kitty::apply()`/
  `clear()` (or an equivalent explicit match); never silently discard a
  `Result` with a bare `let _ =`. A Kitty failure should be logged, not
  invisible.
- `ProcessKittyClient` propagates real errors (including a nonzero
  `kitten` exit, not just a spawn failure). The "never fatal" policy
  belongs at the call site (where it can be logged), not inside the
  client swallowing its own errors. Don't reintroduce that swallowing.
- No `unsafe` except where the standard library itself requires it
  (`std::env::set_var`/`remove_var` in tests, per this edition).

## Testing

- `cargo test --all-targets` before committing.
- Unit tests for anything involving `KittyClient` calls should use
  `kitty::fake::FakeKittyClient`, not spin up real processes.
  `FakeKittyClient.tab_title` behaves like a real tab (`set_tab_title`
  updates it, `get_tab_info` reads it back), so icon-stripping across
  repeated `apply()` calls can be tested the same way the real Kitty
  round-trip works. Use `with_initial_title(...)` to seed it, and
  `with_focused(...)` to seed focus state (defaults to unfocused).
- `icon.rs`'s build/strip functions are pure (no I/O) and should stay
  that way. Test them directly, including round-trip cases (build then
  strip), not just through `kitty::apply()`.
- `daemon::resume_watch` and anything else with `tokio::time::sleep`
  loops should be tested with `#[tokio::test(start_paused = true)]` +
  `tokio::time::advance(...)`, exercising the real interval/debounce/
  timeout logic in milliseconds of wall-clock test time, not by actually
  waiting.
- Integration tests (`tests/*.rs`) that mutate process-global env vars
  (`PATH`, `HOME`, `KITTY_LISTEN_ON`) must do so within a single `#[test]`
  function per file. Rust's default parallel test runner shares env
  vars across tests in the same binary, and cargo only gives each
  `tests/*.rs` file its own process, not each test.
- Tests must pass with `kitten` completely absent from `PATH`, since CI
  has no Kitty installed. Any code path that shells out to `kitten` must
  degrade gracefully (log + continue), never panic or hang.
- `sound_for_transition` is pure and tested directly (`transitions.rs`'s
  `sound_tests` module); no need to exercise real playback to cover the
  decision logic. `sound::play` itself is a no-op under `cfg!(test)`, so
  there's nothing to unit-test there beyond the pure helpers.

## Security

- No network calls, no telemetry, no data collection.
- Hook JSON is parsed in-memory only; the daemon keeps session state in
  memory too. No state files are ever written to disk.
- **`kitten @ ls` (used by `get_tab_info`) returns each matched window's
  full environment variables and other process details alongside its
  title.** `ProcessKittyClient::get_tab_info` must only ever extract the
  `title`/`is_focused` fields from that response. Never log, store, or
  pass through the rest of the parsed JSON or the raw stdout anywhere,
  including `daemon.log` and error messages. (This was not a hypothetical:
  manual testing during development incidentally printed a live API key
  from a window's environment into a terminal session.)
- `daemon.lock`/`daemon.sock`/`daemon.log`/`config.toml` all live under
  `~/.config/kitty-claude-notifier/`. Never write session or hook data
  anywhere else.
- Config parsing is via `serde`/`toml` (no `eval`, no shell interpolation
  of user-controlled config values).
- Sound playback shells out to a fixed allowlist of player binaries with
  static arguments (`PLAYERS` in `sound.rs`). The only user-controlled
  input is the file path (either a config-supplied custom path or a
  tempfile this process just wrote itself), passed as a single `arg()`,
  never through a shell.

## Daemon

- Single process per user, gated by a real OS advisory lock
  (`fd-lock`, held for the daemon's whole lifetime) rather than a
  PID-file/lock-age heuristic. A second daemon that loses the race logs
  it and exits; it never touches the socket file.
- No persistence: every hook event carries full session context, so a
  crashed daemon self-heals from the next hook fire. This is a
  deliberate design choice, not a gap to fill in later.
- `resume_watch`'s poll interval is `config.resume_poll_interval_ms`
  (default 500ms), cheap (one `kitten @ get-text` IPC round-trip), but
  don't reduce it aggressively without considering the cost across many
  concurrently-stuck sessions (each poll is a real process spawn).
- Config is reloaded fresh from disk for every message (`server.rs`'s
  `load_config`), not loaded once at daemon startup. This was a real bug:
  the daemon used to hold one `Config` for its entire lifetime, so an
  edit to `config.toml` had no effect until the daemon was killed and
  respawned, which was confusing in practice since nothing else about
  this daemon requires a restart to pick up changes. A parse error falls
  back to `Config::default()` for that one message rather than failing
  it (e.g. a transient invalid state mid-edit). Already-running
  `idle_timer`/`resume_watch` tasks keep the config snapshot they were
  spawned with; only a session's *next* message picks up an edit.

## Constraints (do not do)

- Do not change the state model without approval (see table above).
- Do not let `hook/` call `kitty::` directly (breaks the "resolve here,
  apply there" separation that makes `daemon/` testable and centralizes
  all Kitty IPC in one place).
- Do not swallow `KittyClient` errors inside the client implementation.
  Swallow (with logging) at the call site instead.
- Do not log or retain any part of `kitten @ ls`'s response beyond the
  `title` field: it contains full environment variables.
- Do not default an icon's color to match its state's tab-background
  color: the icon becomes invisible against its own background.
- Do not use emoji for the *built-in default* icon glyphs: they ignore
  the ANSI foreground color, silently defeating per-state color
  customization for anyone relying on the defaults. (A user's own config
  can still set an emoji glyph if they don't care about `color`; nothing
  enforces this, see the Icon prepending section above.)
- Do not add file-based session persistence without a concrete need:
  the self-healing-from-next-hook-event property is intentional.
- Do not go back to loading `Config` once at daemon startup; reload it
  per message (see the Daemon section above). This was a real bug users
  hit in practice.
- Do not commit code that isn't `cargo fmt`-clean or that introduces new
  `cargo clippy -D warnings` findings.
- Do not assume Kitty/`kitten` is installed in any test; CI doesn't have
  it.
- Do not turn `sound::play`'s `cfg!(test)` guard into a runtime/config
  check; it must stay compile-time so no test build can ever spawn real
  audio playback.
- Do not replay a sound for a repeated/duplicate transition into the same
  state: `sound_for_transition` must keep comparing against `old_state`.
- Do not play a sound for a currently-focused tab: the focus check in
  `transitions::apply()`'s sound branch must stay wired.

## Adding new hook events

1. Add the `(event_key, matcher, our_event, our_matcher)` entry to the
   `HOOKS` table in `install.rs`.
2. Map it in `hook::event::resolve_state` (or handle it specially in
   `hook::run`, like `session-end`'s `MessageKind::Cleanup`).
3. Add a unit test in `hook/event.rs`'s test module.
4. Update the state model table above if it changes a state's
   entry/exit trigger.

## PR guidelines

- `cargo test --all-targets`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo fmt --check` must all pass before pushing; CI enforces the
  same three.
- One feature/fix per PR; keep diffs reviewable.
- Bump `version` in `Cargo.toml` for functional changes (not pure docs).

## Stack

- **Language:** Rust 2024 edition.
- **Async runtime:** tokio (`rt-multi-thread`, `net`, `time`, `process`, `sync`).
- **IPC:** Unix domain socket (NDJSON) between `hook` and `daemon`;
  `kitten @ ... --to unix:$KITTY_LISTEN_ON` for Kitty remote control.
- **Locking:** `fd-lock` (real OS advisory lock for daemon-spawn safety).
- **Serialization:** `serde`/`serde_json` (IPC + hook JSON + parsing
  `kitten @ ls` output), `toml` (config).
- **Logging:** `tracing`/`tracing-subscriber`, to `~/.config/kitty-claude-notifier/daemon.log`
  (and to stderr for the `test` subcommand).
- **CLI:** `clap` (derive).
- **Sound:** no audio library, shells out to a system player
  (`afplay`/`paplay`/`pw-play`/`ffplay`/`mpg123`/`mpv`); built-in MP3s
  embedded via `include_bytes!`, sourced from `herdrdev/herdr` (Apache
  License 2.0, see `assets/sounds/NOTICE.md`).
- **CI:** GitHub Actions: `fmt --check`, `clippy -D warnings`, `test`, on every push/PR.
