# kitty-claude-notifier

[![CI](https://github.com/gbcreation/kitty-claude-notifier/actions/workflows/ci.yml/badge.svg)](https://github.com/gbcreation/kitty-claude-notifier/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Reactive Kitty terminal tab indicators for [Claude Code](https://github.com/anthropics/claude-code).**
See at a glance, across every tab, which of your Claude Code sessions
need your attention, so you never have to tab through every window just
to check.

> [!Note]
> **Vibe-coded:** this project was built through conversational
> pair-programming with [Claude Code](https://claude.com/claude-code).

## Table of contents

- [Features](#features)
- [Requirements](#requirements)
- [Install](#install)
- [Configuration](#configuration)
- [Sound notifications](#sound-notifications)
- [Why this exists](#why-this-exists)
- [How it works](#how-it-works)
- [Acknowledgements](#acknowledgements)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Live tab state**: a small colored icon prepended onto each tab's
  existing title shows whether that session is working, needs a
  permission decision, is waiting on you, compacting its context, just
  finished, or hit an error, without ever clobbering your shell's own
  title.
- **Reactive permission detection**: Claude Code has no hook event for
  "a permission prompt was just resolved," so most tools get stuck
  showing "needs permission" long after you've already answered it. This
  one reads the tab's actual rendered screen content and flips back the
  moment the prompt disappears, typically within 500ms.
- **Optional sound notifications**: an audible cue when a session needs
  your input or just finished, automatically suppressed for whichever
  tab you're currently looking at.
- **Per-state colors and icons**: fully configurable, including
  separate colors for a tab's focused vs. unfocused background.
- **No extra runtime dependencies**: a single static binary plus
  whatever's already on your system (Kitty, optionally a system audio
  player). Config changes apply live, no restart needed.
- **Privacy-conscious**: never logs or retains anything beyond a tab's
  title and focus state, even though the underlying Kitty IPC call
  exposes far more.

## Requirements

- [Kitty](https://sw.kovidgoyal.net/kitty/) terminal, with
  `allow_remote_control yes` and a `listen_on` socket configured in
  `kitty.conf` (e.g. `listen_on unix:/tmp/kitty-{kitty_pid}`). Without
  this, `kitten @` can't reach Kitty from a hook subprocess, which has
  no controlling TTY.
- [Claude Code](https://github.com/anthropics/claude-code).
- Rust toolchain to build (no prebuilt binaries yet).
- *Optional:* a system audio player for sound notifications:
  `afplay` (macOS, built in) or `paplay`/`pw-play`/`ffplay`/`mpg123`/`mpv`
  (Linux, tried in that order).

## Install

```bash
cargo build --release
./target/release/kitty-claude-notifier install
```

This copies the binary to `~/.config/kitty-claude-notifier/bin/`, writes
a default config if none exists, and appends its own hook entries into
`~/.claude/settings.json`. It never touches or removes any other
entries already in that file (verified: installing then uninstalling
round-trips the settings file byte-for-byte).

To uninstall (removes only this tool's hook entries and copied binary;
your `config.toml` is left in place):

```bash
kitty-claude-notifier uninstall
```

## Configuration

`~/.config/kitty-claude-notifier/config.toml` (see
[`config/default.toml`](config/default.toml) for the shipped defaults;
any key you omit falls back to its built-in default). Edits take effect
on the very next hook event, since the daemon reloads the file fresh
for every message and needs no restart:

```toml
idle_timeout_secs = 300          # done/waiting/compacting -> idle
resume_poll_interval_ms = 500    # get-text poll rate while blocked

permission_markers = [           # screen-text markers meaning "still blocked"
    "do you want to proceed?",
    "❯ 1. ",                    # numbered-menu prefix, not "❯ 1. yes":
]                                # the suggested option's wording varies

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
# sound_events = ["permission", "waiting", "done"]  # which states play a
                                  # sound; restrict to e.g. ["done"] to
                                  # only hear about finished turns
sound_play_when_focused = false  # off by default; suppresses the sound
                                  # for the tab you're already looking at
# [sounds]                       # optional custom sound file paths;
# request = "/path/to/request.mp3"  # omit either to use the built-in default
# done = "/path/to/done.mp3"
```

**`sound_enabled` (and any other top-level key) must stay above every
`[table]` header.** TOML assigns a key appearing after a `[table]`
header to that table, not to the document root. Appending a line with
`>>` to the end of the file is the easy way to get this wrong (a real
mistake made once during development: it silently landed inside
`[colors.error]` instead of at the root, and was dropped by serde
without any parse error).

Icon glyphs should be **plain Unicode symbols or Nerd Font glyphs, not
emoji**, if you want the `color` setting to actually apply. Emoji render
in the tab title just fine, but they carry their own embedded color and
ignore `color` entirely: a colored emoji icon silently renders in its
default color instead (confirmed by testing).

If Claude Code's prompt wording changes across versions and resume
detection stops firing, update `permission_markers`; no rebuild needed.

## Sound notifications

Off by default. When `sound_enabled = true`, a sound plays on entering
`permission`/`waiting` (needs your input) and on `done` (a turn
finished), the states listed in `sound_events` (all three by default;
restrict it to hear about only some, e.g. `sound_events = ["done"]`).
Nothing plays for repeated/no-op transitions into the same state, and
nothing plays if the tab is currently focused (you're already looking
at it) unless `sound_play_when_focused = true`. Playback runs in a
background thread and never blocks or fails the state update itself.

The two built-in sounds are sourced from
[herdrdev/herdr](https://github.com/herdrdev/herdr) (Apache License
2.0); see [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md) for
attribution. Override either with `[sounds] request = "..."` /
`done = "..."` in `config.toml`; a custom path that fails to play falls
back to the built-in default.

Requires a system audio player on `PATH`. Silently does nothing if none
are found. Check `~/.config/kitty-claude-notifier/daemon.log` for a
`sound playback failed` warning if you enable it and hear nothing.

## Why this exists

Claude Code fires hook events for most session-state transitions (working,
permission needed, waiting on you, done, error), but it has no event for
"a permission prompt was just resolved." A hook-only design leaves a tab
stuck showing "permission needed" until the next unrelated hook happens to
fire and reset it, which could be a long time.

This tool closes that gap directly: while a session sits in
`permission`, a background daemon polls the tab's actual rendered
screen content (`kitten @ get-text`) and watches for the prompt text to
disappear. The moment it does, the tab flips back to `working`,
typically within one poll interval (500ms by default) rather than
whenever the next hook happens to arrive. Every other state transition,
including `waiting`'s resolution once you actually reply, still comes
straight from Claude Code's hooks, which are already fast and precise;
this only replaces the one mechanism that couldn't be hook-driven (a
permission dialog is answered via a menu selection, which fires no hook
at all).

Rather than overwriting a tab's title outright, it prepends a small
colored icon onto whatever the title already is, so your shell's own
title (current directory, running command, etc.) stays visible, with just
a glyph in front of it signaling state.

## How it works

```
Claude Code hook fires
  → kitty-claude-notifier hook --event <name> [--matcher <m>] --stdin
    → resolves the target Kitty window + state here (only meaningful
      in the hook process's own environment)
    → sends one message to the daemon over a Unix socket,
      spawning it (detached) first if nothing answers

daemon (long-running, tokio)
  → holds per-session state in memory (no state files, since every hook
    event carries full context, so a crash self-heals from the next one)
  → reloads config.toml fresh from disk for every message (no restart
    needed to pick up an edit)
  → owns all Kitty IPC centrally:
      - fetches the tab's current title (unless a fixed `text` override
        is configured), strips any icon this tool previously applied,
        and prepends the new one, never replacing the rest of the title
      - sets the tab background color
  → per-session cancellable timers:
      - idle_timer: exact-timing done/waiting/compacting -> idle (a
        safety net for compacting, in case PostCompact never fires)
      - resume_watch: polls get-text while permission (a menu-selection
        dialog Claude Code fires no hook for on resolution), flips to
        working once the configured markers disappear for two
        consecutive polls (debounced against burst prompts). waiting
        resolves through a real hook instead (typically UserPromptSubmit
        once you reply), since its on-screen content varies too much
        for marker-matching to work reliably
  → plays a sound (if sound_enabled, the entered state is in
    sound_events, and the tab isn't currently focused) on entering
    permission/waiting or done, in a background thread, via whichever
    system audio player happens to be installed
```

A real OS advisory lock (`fd-lock`) gates daemon startup, so two
concurrently-spawned daemon processes can't race to bind the socket:
one binds, the other logs that it lost the race and exits.

**Note on privacy:** reading a tab's current title (and focus state, used
to suppress sounds for a tab you're already looking at) uses `kitten @
ls`, which returns each matched window's full environment variables and
other process details alongside its title. This tool only ever extracts
the `title`/`is_focused` fields from that response and never logs or
retains the rest.

## Acknowledgements

- [**claude-notifier**](https://github.com/omendivil/claude-notifier):
  the bash predecessor to this project, giving the same visual
  tab-state idea for Claude Code + Kitty. This project is a from-scratch
  Rust rewrite, built specifically to close the "stuck permission tab"
  gap that its hook-and-poll approach couldn't fully solve.
- [**herdr**](https://github.com/herdrdev/herdr): a Claude Code runtime
  whose approach of reading the terminal's actual rendered screen text
  (rather than relying solely on hooks) to detect agent state directly
  inspired this project's reactive resume-detection design. The
  built-in notification sounds are also sourced from herdr; see
  [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md).

## Contributing

Issues and pull requests are welcome. Before opening a PR:

```bash
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

CI (`.github/workflows/ci.yml`) runs all three on every push/PR; see
[`CLAUDE.md`](CLAUDE.md) for the full architecture and conventions this
project follows.

## License

Licensed under the [MIT license](LICENSE). The bundled notification
sounds (`assets/sounds/*.mp3`) are sourced from
[herdrdev/herdr](https://github.com/herdrdev/herdr) and remain under the
[Apache License 2.0](assets/sounds/LICENSE-APACHE-herdr); see
[`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md) for attribution.
