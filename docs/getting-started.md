# Getting Started

## Configuration

Skeld supports user-wide configurations, where common whitelists, the
colorscheme, the default editor/IDE, etc. can be configured. To do so, create a
file at `$XDG_CONFIG_HOME/skeld/config.toml` with content similar to:
```toml
[colorscheme]
normal = "#DCD7BA"
banner = "#E6C384"
heading = "#C0A36E"
project-name = "#727169"
keybind = "#6A9589"
background = "#1F1F28"

# User-wide whitelists.
[project]
# Read-write whitelists.
whitelist-rw = [
  "$(DATA)/nvim",
  "$(STATE)/nvim",
]
# Read-only whitelists.
whitelist-ro = [
  "~/.bashrc",
  "$(CONFIG)/nvim",

  "/usr",
  "/etc",
]
# Whitelisted symlinks.
# NOTE: Depending on the system, these may not be symlinks; in which case they
#       should be put in `whitelist-ro`.
whitelist-ln = [
  "/bin",
  "/lib",
  "/lib64",
]
add-tmpfs = [
  "/tmp",
]
# If desired, `whitelist-envvar` can be used to whitelist only specific
# environment variables.
whitelist-all-envvars = true

# Set the default editor/IDE.
[project.defaults.editor]
# NOTE: `$(FILE:.)` is replaced with the initial file if one is provided;
#       otherwise, it's replaced with `.`
cmd = [ "nvim", "$(FILE:.)" ]
# Whether to detach the editor from the terminal. Should be true for GUI editors
# and false for TUI editors.
detach = false
```
Refer to `man 'skeld-config(5)'` for more information.

## Projects

To add a project, create a file at
`$XDG_DATA_HOME/skeld/projects/<Project Name>.toml` with content similar to:
```toml
[project]
project-dir = "<Project Directory>"
# Optionally, a file to open initially can be specified.
initial-file = "src/main.rs"

# Project-specific whitelists.
whitelist-dev = [
  "/dev/dri/",
]

# Including options from other files is supported. Include files are searched in
# $XDG_DATA_HOME/skeld/include/ or $XDG_CONFIG_HOME/skeld/include/.
include = [ "..." ]
```
Refer to `man 'skeld(1)'` for more information.

## Bookmarks

Bookmarks use the same file format as projects, but must be placed into the
`bookmarks/` directory instead of `projects/`.
