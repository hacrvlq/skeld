# Skeld

> Programming is constant remote code execution.[^1]

Or have you inspected all your dependencies? It's easy to write a
library that steals private ssh keys on the side.

Skeld mitigates this risk by opening projects in a **restricted sandbox**
where only the required paths are accessible.
A sandbox can be conveniently created via a **terminal UI** by selecting a
project, which is then opened in the configured editor/IDE (inside a sandbox).

![screenshot of the skeld tui](screenshot.png)

The paths that the sandbox can access are defined on a per-project basis:
```toml
# projects are specified with a single TOML file

[project]
project-dir = "~/dev/skeld"
# paths can be whitelisted read-only
whitelist-ro = [
  # some string interpolation is supported
  # (see `man 'skeld(7)'`, section "String Interpolation")
  "$(CONFIG)/nvim",
]
# paths can be whitelisted read-write
whitelist-rw = [
  "$(DATA)/nvim",
  "$(STATE)/nvim",
]
# including options from other files is also supported
include = ["rust"]
```

## Installation
Note that only Linux is supported.

> [!IMPORTANT]
> Skeld depends on [Bubblewrap](https://github.com/containers/bubblewrap), so it must be available in `PATH`.

- Pre-built binaries: **[Releases](https://github.com/hacrvlq/skeld/releases)**
- Using [Cargo](https://www.rust-lang.org/tools/install): `cargo install skeld`

## Getting Started
Without any configuration, the skeld UI displays a blank screen. Some
configuration is therefore inevitable. Below is an example configuration for
the [neovim](https://neovim.io) editor.
### Configuration
Create a file `$XDG_CONFIG_HOME/skeld/config.toml` with the following content:
```toml
# it is possible to disable the help text in the bottom right corner
disable-help = false

# colorscheme from the screenshot
[colorscheme]
normal = "#DCD7BA"
banner = "#E6C384"
heading = "#C0A36E"
label = "#727169"
keybind = "#6A9589"
background = "#1F1F28"

[[commands]]
name = "<edit>"
keybind = "e"
command = ["nvim"]
# whether to detach from terminal;
# should be true for GUI commands and false for TUI commands
detach = false

[[commands]]
name = "<quit>"
keybind = "q"
# if 'command' is empty, skeld quits immediately
command = []

# user-wide whitelists
[project]
# read-write whitelists
whitelist-rw = [
  "$(DATA)/nvim",
  "$(STATE)/nvim",
]
# read-only whitelists
whitelist-ro = [
  "~/.bashrc",
  "$(CONFIG)/nvim",

  "/usr",
  "/etc",
]
# symlink whitelists
# NOTE: depending on the system, these may not be symlinks;
#       so they may need to be in 'whitelist-ro'
whitelist-ln = [
  "/bin",
  "/lib",
  "/lib64",
]
add-tmpfs = [
  "/tmp",
]
# as long as no secrets are stored in environment variables,
# this should be fine
whitelist-all-envvars = true

# configure the editor/IDE used to open projects
[project.editor]
# command used when a project specifies a file to be opened initially
cmd-with-file = ["nvim", "$(FILE)"]
# command used when no initial file is specified
cmd-without-file = ["nvim", "."]
# whether to detach editor from terminal;
# should be true for GUI editors and false for TUI editors
detach = false
```
Refer to `man 'skeld(7)'` for all supported options.

### Projects
To add a project, create a file at
`$XDG_DATA_HOME/skeld/projects/<your_project_name>.toml`
with the following content:
```toml
[project]
project-dir = "<your_project_directory>"
# optionally, a file to be opened initially can be specified
initial-file = "src/main.rs"

# project-specific whitelists
whitelist-dev = [
  "/dev/dri/",
]
# Language-specific whitelists can be separated into different a file.
# To do so, create a file at $XDG_DATA_HOME/skeld/include/<your_lang>.toml
# with the language-specific whitelists.
include = ["<your_lang>"]
```
Refer to `man 'skeld(7)'` for all supported options.

## Documentation
The documentation is available at `man 'skeld(7)'`

## Building
Requires the [Rust Compiler](https://www.rust-lang.org/tools/install).
```sh
cargo build --release
./target/release/skeld
```
To build the man page, [scdoc](https://git.sr.ht/~sircmpwn/scdoc) is required.
```sh
scdoc < docs/skeld.7.scd > skeld.7
```

## License
Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE))
* MIT license ([LICENSE-MIT](../LICENSE-MIT))

at your option.

## Contribution
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[^1]: This might be slightly overdramatized.
