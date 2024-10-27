# Documentation

- [Usage](#usage)
- [Project Data Format](#project-data-format)
  - [String Interpolation](#string-interpolation)
  - [File Locations](#file-locations)
- [Projects](#projects)
- [Bookmarks](#bookmarks)
- [Configuration](#configuration)

## Usage
```
Usage: skeld [COMMAND]

Commands:
  ui    Open the skeld tui
  add   Add a project

(Use `skeld --help` to show all options)
```

## Project Data Format
This is the core format that describes everything that is needed to open a
project, including the configuration of the sandbox. The format used is
[TOML](https://toml.io) with the following supported options:
```toml
# root directory of the project
project-dir = "..."
# file to be opened initially (optional)
# NOTE: the provided path should be relative to 'project-dir'
initial-file = "..."
# automatically open project in nix-shell if 'shell.nix' or 'default.nix' exists
auto-nixshell = true # Default: false
# disable the sandbox altogether
no-sandbox = true # Default: false

# whitelist paths read-write
whitelist-rw = [
  # some string interpolation is supported (see #String-Interpolation)
  "$(DATA)/nvim",
]
# whitelist paths read-only
whitelist-ro = [ "..." ]
# whitelist paths allowing device access
whitelist-dev = [ "..." ]
# whitelist symlinks (copies symlinks as-is into the sandbox)
whitelist-ln = [ "..." ]
# mount a tmpfs to the specified paths
add-tmpfs = [ "..." ]

# if 'whitelist-all-envvars' is true, all environment variables remain accessible;
# otherwise only the variables in 'whitelist-envvar' are transferred into the sandbox
whitelist-all-envvars = true # Default: false
whitelist-envvar = [ "..." ]

# include options from other files
# NOTE: circular includes are allowed
include = [
  # relative paths are searched in <SKELD-DATA>/include
  # (see #File-Locations)
  # NOTE: The `toml` file extension will be appended to the
  #       specified path.
  "rust",
  # absolute paths are also supported
  "/etc/system.toml",
]

# editor used to open the project
[editor]
# used when 'initial-file' is set
# NOTE: '$(FILE)' will be replaced with the value of 'initial-file'
cmd-with-file = ["nvim", "$(FILE)"]
# used when 'initial-file' is not set
cmd-without-file = ["nvim", "."]
# whether to detach editor from terminal,
# when true 'skeld' terminates after project has been opened
# NOTE: should be true for GUI editors and false for TUI editors
detach = false
```

### String Interpolation
Wherever a path is expected, the following placeholders can be used:
| Placeholder        | Substitution |
| ------------------ | ------------ |
| `$[ENVVAR]`        | value of environment variable `ENVVAR` |
| `$[ENVVAR:ALTVAL]` | value of environment variable `ENVVAR` if existent, otherwise `ALTVAL` |
| `$(CONFIG)`        | `XDG_CONFIG_HOME` if existent, otherwise `~/.config` |
| `$(CACHE)`         | `XDG_CACHE_HOME` if existent, otherwise `~/.cache` |
| `$(DATA)`          | `XDG_DATA_HOME` if existent, otherwise `~/.local/share` |
| `$(STATE)`         | `XDG_STATE_HOME` if existent, otherwise `~/.local/state` |

### File Locations
Skeld searches for project/configuration files in:

- `$XDG_CONFIG_HOME/skeld` (fallback `~/.config/skeld`)
- `$XDG_DATA_HOME/skeld` (fallback `~/.local/share/skeld`)

These locations are referred to as `<SKELD-DATA>`.

## Projects
Project files are located in `<SKELD-DATA>/projects`. Note that files need the
extension `toml` in order to be recognized.
See [Project Data Format](#project-data-format) for supported options.

## Bookmarks
Bookmark files are located in `<SKELD-DATA>/bookmarks`. They must have the
extension `toml` and the following content:
```toml
name = "nvim-config"
keybind = "cv"

[project]
# see #Project-Data-Format for supported options
```

## Configuration
The configuration is located at `$XDG_CONFIG_HOME/skeld/config.toml` (fallback
`~/.config/skeld/config.toml`). The following options are supported:
```toml
# banner shown at the top
# NOTE: example was generated with figlet using larry3d font
banner = '''
                               __
  ___      __    ___   __  __ /\_\    ___ ___
/' _ `\  /'__`\ / __`\/\ \/\ \\/\ \ /' __` __`\
/\ \/\ \/\  __//\ \_\ \ \ \_/ |\ \ \/\ \/\ \/\ \
\ \_\ \_\ \____\ \____/\ \___/  \ \_\ \_\ \_\ \_\
 \/_/\/_/\/____/\/___/  \/__/    \/_/\/_/\/_/\/_/
'''
# disable the help text in the bottom right corner
disable_help_text = true

[colorscheme]
# colors can be specified as hex color codes
neutral = "#DCD7BA"
# or as ansi color codes (see https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit)
banner = 3
heading = "#C0A36E"
label = "#727169"
keybind = "#6A9589"

[[commands]]
name = "<edit>"
keybind = "e"
# if 'command' is empty, skeld quits immediatly
command = ["nvim", "--clean"]
# see 'detach' in #Project-Data-Format
detach = false

# user-wide project data that is merged with per-project data
[project]
# see #Project-Data-Format for supported options
```
