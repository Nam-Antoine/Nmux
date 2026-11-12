# Nmux

A persistent desktop home for Claude Code. Sessions run in real terminals that
belong to Nmux, so closing VS Code (or the Nmux window) never kills them.

Nmux is written in Rust and modelled on [Herdr](https://herdr.dev): a
workspace per project, tabs inside it, split panes inside a tab, and a state
for every agent (needs you, working, done, idle).

- Panes run in a background server, so closing the window stops nothing. The
  layout is saved; after a restart panes come back stopped and resume on request.
- Resume: Nmux owns each Claude session ID, so a stopped pane continues its
  conversation. If there is nothing to resume, it starts a new one.
- Cloud tasks: `claude --cloud "<task>"` from a workspace, then "Teleport" to
  bring the cloud session into a local pane.
- Files panel: the workspace folder as a tree, with preview, open, reveal and
  "type this path into the pane". Right-click a row (or the empty space) for
  new file, new folder, rename, delete, copy path and upload; `F2` renames and
  `Delete` deletes the focused row. Upload picks files or folders, or drop
  them onto a folder in the tree; if names are taken it lists them and asks
  before going on (folders are merged, same-named files replaced).
  Delete goes to the Recycle Bin on this computer. The workspace folder itself
  cannot be renamed or deleted.
- Ports panel: ports your panes listen on (open in the browser), and SSH port
  forwards (`ssh -L`) to other machines.
- Changes panel: git branch, changed files with line counts, and diffs.
- Paste a copied image, or drop files, into a pane: the agent gets the path.
- Machines: "Add machine" lists the hosts in your `~/.ssh/config` (or type a
  target), checks that it connects, and you open workspaces on it next to the
  local ones. A remote pane is your own `ssh` in a terminal, so your
  config, keys and prompts work as usual, and nothing is installed there. With
  tmux on the machine, panes keep running when the connection drops or Nmux
  closes; starting the pane again reattaches. Ports, Changes and Files work
  there too, and Files can go to any folder on the machine. Wherever you type
  a folder on a machine (a new workspace, the Files path bar), Nmux suggests
  the folders that match as you type, at any depth: arrows to choose, `Tab` or
  `Enter` to complete. Files on a machine has the same actions, plus Download,
  which saves a file or folder into this computer's Downloads folder. Uploads
  and downloads go through your `ssh` and need `tar` on the machine. Delete on
  a machine is permanent.
- Run `claude` in a shell pane and it is shown and tracked as a Claude Code
  pane until Claude exits. If Nmux closes first, the pane resumes that
  conversation.
- What was running when Nmux shut down starts again when it opens: Claude
  panes resume their conversations, shells open in their folders.
- Keyboard: `Ctrl+B` then a key (`?` lists them). Everything is on the mouse too.

## nc: talk to panes from a terminal

`nc` is Nmux's command-line tool (it is not netcat). Every workspace has a
number, shown in the pane header: `n8` is the first pane of workspace 8, `n8b`
the second. A workspace name works as well.

```bash
nc ls                              # running panes and their addresses
nc send n8 "what port does the API use?"
nc wait n8 && nc read n8           # wait for the agent, read its screen
nc run --bg claude "review src/x.rs"
nc --help
```

A message sent from one pane to an agent starts with who sent it and how to
reply, so agents in different workspaces can ask each other questions. `nc` is
on the PATH of every pane.

## Layout

```
crates/nmux-core     model, split layout, agent detection, input validation (no I/O)
crates/nmux-proto    messages between server and clients
crates/nmux-server   background server that owns the terminals; the nc client
src-tauri/           the window: a thin Tauri shell over the server
ui/                  React + xterm interface
```

## Build

Requires Rust, Node 22 or newer, pnpm 10, and `claude` on PATH.

```bash
cd ui && pnpm install && pnpm build && cd ..
cargo build --workspace && ./target/debug/nmux.exe
cargo test --workspace
bash scripts/dist-rs.sh        # installer in target/release/bundle/nsis/
```

`NMUX_DATA_DIR=<folder>` runs against a separate data folder. State lives in
`%APPDATA%/nmux-rs` otherwise. Claude keeps the transcripts in
`~/.claude/projects`.

Windows 11 is the tested platform. The earlier Electron version is in the git
history before commit `08d4b32`. Agent detection rules are a reduced port of
Herdr's (Apache-2.0).

## License

[MIT](LICENSE) © 2026 Nathan Collins
