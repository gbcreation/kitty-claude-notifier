# kitty-claude-notifier

Reactive Kitty terminal tab indicators for Claude Code.

## Why this exists

Claude Code fires hook events for most session-state transitions (working,
permission needed, waiting on you, done, error), but it has **no event for
"a permission prompt was just resolved."** A hook-only design leaves a tab
stuck showing "permission needed" until the next unrelated hook happens to
fire and reset it — which could be a long time.

This tool closes that gap directly: while a session sits in
`permission`/`waiting`, a background daemon polls the tab's actual
rendered screen content (`kitten @ get-text`) and watches for the prompt
text to disappear. The moment it does, the tab flips back to `working` —
typically within one poll interval (500ms by default), not whenever the
next hook happens to arrive. Every other state transition still comes
straight from Claude Code's hooks, which are already fast and precise;
this only replaces the one mechanism that couldn't be hook-driven.

Rather than overwriting a tab's title outright, it **prepends a small
colored icon** onto whatever the title already is — so your shell's own
title (current directory, running command, etc.) stays visible, with just
a glyph in front of it signaling state.

## Install

```bash
cargo build --release
./target/release/kitty-claude-notifier install
```

`install` copies the binary to `~/.config/kitty-claude-notifier/bin/`,
writes a default config if none exists, and appends its own hook entries
into `~/.claude/settings.json` — it never touches or removes any other
entries already in that file (verified: installing then uninstalling
round-trips the settings file byte-for-byte).

Requires Kitty with `allow_remote_control yes` and a `listen_on` socket
configured (e.g. `listen_on unix:/tmp/kitty-{kitty_pid}`) — without a
listen socket, `kitten @` can only reach Kitty from a process with a
controlling TTY, which hook subprocesses don't have.

## Uninstall

```bash
kitty-claude-notifier uninstall
```

Removes only this tool's hook entries and the copied binary. Your
`config.toml` is left in place.

## Configuration

`~/.config/kitty-claude-notifier/config.toml` (see
[`config/default.toml`](config/default.toml) for the shipped defaults —
any key you omit falls back to its built-in default). Edits take effect
on the very next hook event — the daemon reloads the file fresh for
every message, no restart needed:

```toml
idle_timeout_secs = 300          # done/waiting -> idle
resume_poll_interval_ms = 500    # get-text poll rate while blocked

permission_markers = [           # screen-text markers meaning "still blocked"
    "do you want to proceed?",
    "❯ 1. yes",
]

[icons.permission]                # per-state icon: glyph + color,
glyph = "▲"                       # prepended onto the tab's existing title
color = "#ffffff"
# text = "NEEDS APPROVAL"         # optional: use fixed text instead of the
                                  # live title (skips fetching it entirely)

[colors]   # per-state tab-background color overrides ("NONE" clears it);
           # a plain string applies to both focused/unfocused, or use
           # [colors.STATE] with active/inactive to split them

sound_enabled = false            # off by default; plays a sound on
                                  # entering permission/waiting and on done
sound_play_when_focused = false  # off by default; suppresses the sound
                                  # for the tab you're already looking at
# [sounds]                       # optional custom sound file paths;
# request = "/path/to/request.mp3"  # omit either to use the built-in default
# done = "/path/to/done.mp3"
```

**`sound_enabled` (and any other top-level key) must stay above every
`[table]` header** — TOML assigns a key appearing after a `[table]`
header to that table, not to the document root. Appending a line with
`>>` to the end of the file is the easy way to get this wrong (a real
mistake made once during development: it silently landed inside
`[colors.error]` instead of at the root, and was dropped by serde
without any parse error).

Icon glyphs must be **plain Unicode symbols or Nerd Font glyphs, not
emoji** — emoji carry their own embedded color and ignore the `color`
setting entirely (confirmed by testing; a colored emoji icon silently
just renders in its default color).

If Claude Code's prompt wording changes across versions and resume
detection stops firing, update `permission_markers` — no rebuild needed.

## How it works

```
Claude Code hook fires
  → kitty-claude-notifier hook --event <name> [--matcher <m>] --stdin
    → resolves the target Kitty window + state here (only meaningful
      in the hook process's own environment)
    → sends one message to the daemon over a Unix socket,
      spawning it (detached) first if nothing answers

daemon (long-running, tokio)
  → holds per-session state in memory (no state files — every hook
    event carries full context, so a crash self-heals from the next one)
  → reloads config.toml fresh from disk for every message (no restart
    needed to pick up an edit)
  → owns all Kitty IPC centrally:
      - fetches the tab's current title (unless a fixed `text` override
        is configured), strips any icon this tool previously applied,
        and prepends the new one — never replacing the rest of the title
      - sets the tab background color
  → per-session cancellable timers:
      - idle_timer: exact-timing done/waiting -> idle
      - resume_watch: polls get-text while permission/waiting, flips
        to working once the configured markers disappear for two
        consecutive polls (debounced against burst prompts)
  → plays a sound (if sound_enabled and the tab isn't currently
    focused) on entering permission/waiting or done, in a background
    thread, via whichever system audio player happens to be installed
```

## Sound notifications

Off by default. When `sound_enabled = true`, a sound plays on entering
`permission`/`waiting` (needs your input) and on `done` (a turn
finished) — nothing plays for repeated/no-op transitions into the same
state, and nothing plays if the tab is currently focused (you're already
looking at it) unless `sound_play_when_focused = true`. Playback runs in
a background thread and never blocks or fails the state update itself.

The two built-in sounds are sourced from
[herdrdev/herdr](https://github.com/herdrdev/herdr) (Apache License
2.0) — see [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md) for
attribution. Override either with `[sounds] request = "..."` /
`done = "..."` in `config.toml`; a custom path that fails to play falls
back to the built-in default.

Requires a system audio player on `PATH`: `afplay` on macOS;
`paplay`/`pw-play`/`ffplay`/`mpg123`/`mpv` (tried in that order) on
Linux. Silently does nothing if none are found — check
`~/.config/kitty-claude-notifier/daemon.log` for a `sound playback
failed` warning if you enable it and hear nothing.

A real OS advisory lock (`fd-lock`) gates daemon startup, so two
concurrently-spawned daemon processes can't race to bind the socket —
one binds, the other logs that it lost the race and exits.

**Note on privacy:** reading a tab's current title (and focus state, used
to suppress sounds for a tab you're already looking at) uses `kitten @
ls`, which returns each matched window's full environment variables and
other process details alongside its title. This tool only ever extracts
the `title`/`is_focused` fields from that response and never logs or
retains the rest.

## Development

```bash
cargo test              # 52 tests: unit (FakeKittyClient, no real Kitty
                         # needed) + integration (mock kitten binary, and
                         # the real compiled daemon against a temp socket)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

CI (`.github/workflows/ci.yml`) runs all three on every push/PR.
