# seela

A tmux session manager. Lets you fuzzy-find your projects
and handles the window/pane layout based on your config.

## Installation

```bash
cargo install seela
```

Make sure `$CARGO_HOME/bin` (usually `~/.cargo/bin`) is in your `PATH`.

### Build from source

```bash
git clone https://github.com/BardiyaFeili/seela.git
cd seela
cargo build --release
```

## Usage

```bash
Usage: seela [OPTIONS] [DIR]

Arguments:
  [DIR]  Open a directory as tmux session, config will still apply

Options:
  -c, --config <FILE>  Path to a config file
  -h, --help           Print help
  -V, --version        Print version
```

Bind it to a key in tmux: (In `tmux.conf`)

```tmux
bind g display-popup -w 80% -h 80% -E "seela"
```

## Config file

seela looks for config in this order:

1. `--config` flag
2. `$SEELA_CONFIG_HOME/config.toml`
3. `$XDG_CONFIG_HOME/seela/config.toml`
4. `~/.config/seela/config.toml`

---

## Folders

```toml
[folders]
search_dirs   = ["~/projects", "~/work"]
exclude_paths = ["~/projects/archive"]
force_include = ["~/dotfiles"]
```

- `search_dirs` — walked recursively, any directory with `.git` is a project
- `exclude_paths` — skipped entirely, including subdirectories
- `force_include` — always included, ignores exclusion rules

If a `search_dir` is inside an excluded path, it still gets searched.

```toml
# ~/code/old is excluded, but ~/code/old/needed is still searched
search_dirs   = ["~/code", "~/code/old/needed"]
exclude_paths = ["~/code/old"]
```

---

## fzf

```toml
[fzf]
preview         = true
preview_command = "tree -C -L 2 {}"
fzf_opts        = "--height 40% --layout=reverse"
```

---

## Sessions

When you pick a project, seela matches it to a session config in this order:

1. Exact path match
2. Project type match
3. Closest path prefix
4. `default_session`

```toml
[default_session]
windows      = ["editor", "terminal"]
window_focus = "editor"
```

```toml
[[custom_sessions]]
paths        = ["~/projects/myapp"]   # exact or prefix match
types        = ["rust"]               # or by project type
windows      = ["editor", "bacon"]
window_focus = "editor"
```

`paths` and `types` are OR'd, either one matching is enough.

### Project types

A type matches if any of its `files` exist in the project directory.

```toml
[[project_types]]
name  = "rust"
files = ["Cargo.toml"]

[[project_types]]
name  = "web"
files = ["package.json", "tsconfig.json"]
```

---

## Windows

Windows are defined globally and referenced by name from sessions.

```toml
[[windows]]
name = "editor"

[[windows.panes]]
exec = ["nvim"]
```

### Pane splits

`split` on a pane describes how its children are arranged:

- `"vertical"` — side by side
- `"horizontal"` — top and bottom

`ratio` controls proportional size. Omit it for equal splits.

```toml
[[windows]]
name = "dev"

[[windows.panes]]
split = "vertical"

  [[windows.panes.panes]]
  exec  = ["nvim"]
  ratio = 0.7

  [[windows.panes.panes]]
  split = "horizontal"
  ratio = 0.3

    [[windows.panes.panes.panes]]
    exec  = ["cargo watch"]
    ratio = 0.6

    [[windows.panes.panes.panes]]
    exec  = ["lazygit"]
    ratio = 0.4
```

---

## Exec operators

Used inside a pane's `exec` list to control how commands run.

### `@confirm`

Prompts before running. Good for destructive or slow commands.

```toml
exec = ["@confirm cargo run --release"]
# Run "cargo run --release"? [Y/n]
```

### `@run`

Runs a script or command with these environment variables passed to it:

| Variable             | Value                        |
| -------------------- | ---------------------------- |
| `SEELA_SESSION_PATH` | Absolute path to the project |
| `SEELA_SESSION_NAME` | tmux session name            |
| `SEELA_WINDOW_NAME`  | Current window name          |
| `SEELA_PANE_ID`      | tmux pane ID                 |

Supports `~` and paths relative to the config file.

```toml
exec = ["@run ~/scripts/setup.sh"]
exec = ["@run scripts/build.sh"]   # relative to config file
exec = ["@run notify-send done"]   # plain commands work too
```

### `@send-key` / `@sk`

Sends a raw key sequence to the pane. Useful for interacting with a running app.

```toml
exec = ["nvim", "@sk g"]   # open neovim, then sends 'g'
```

Keys follow tmux key names: `Enter`, `Escape`, `C-c`, `C-l`, `Space`, etc.

### `@wait` / `@wait-milli`

Pauses before the next command.

```toml
exec = ["start-server", "@wait 2", "connect-client"]
exec = ["start-server", "@wait-milli 500", "connect-client"]
```

### Example combining operators

```toml
[[windows]]
name = "app"
[[windows.panes]]
split = "vertical"

  [[windows.panes.panes]]
  exec = [
    "@confirm RUST_LOG=debug cargo run",
  ]

  [[windows.panes.panes]]
  exec = [
    "@confirm tail -f /tmp/app.log",
  ]
```

---

## Window hooks

Scripts that run when a window opens, not tied to any pane.
Good for background tasks, notifications, or setup that doesn't need a terminal.

```toml
[[windows]]
name   = "editor"
hooks  = ["notify-send 'opened' '$SEELA_SESSION_NAME'"]

[[windows.panes]]
exec   = ["nvim"]
```

These environment variables are passed to hooks:

| Variable             | Value                        |
| -------------------- | ---------------------------- |
| `SEELA_SESSION_PATH` | Absolute path to the project |
| `SEELA_SESSION_NAME` | tmux session name            |
| `SEELA_WINDOW_NAME`  | Current window name          |

By default hooks run sequentially. Set `hooks_parallel = true` to run them in parallel.

```toml
[[windows]]
name           = "editor"
hooks_parallel = true
hooks          = [
  "notify-send 'ready' 'editor'",
  "~/scripts/sync.sh",
]
```

Supports `~`, paths relative to the config file, and plain commands.

If a hook fails, seela prints the exit code and stderr to your terminal.
Successful hooks are silent.

### Example: desktop notification on open

```bash
#!/bin/env bash
# ~/.config/seela/scripts/notify.sh
notify-send "Opened $SEELA_SESSION_NAME" "$SEELA_SESSION_PATH"
```

```toml
[[windows]]
name  = "editor"
hooks = ["scripts/notify.sh"]

[[windows.panes]]
exec  = ["nvim"]
```

---

## Timing

seela uses shell readiness polling to know when a pane is ready for the next command.
You can tune them if commands are firing too early.

```toml
[tmux]
startup_delay_ms = 600   # wait before sending any commands
key_delay_ms     = 60    # delay between keystrokes
action_delay_ms  = 200   # delay after Enter
```
