SKELD(7)

# NAME

skeld - a tui tool for opening projects in a restricted sandbox to prevent
supply chain attacks such as typosquatting

# USAGE

*skeld ui*
	Launch the skeld tui

*skeld add* _<path>_
	Add _<path>_ to the projects

(Use *skeld --help* to show all options)

# FILES
Skeld searches for project/configuration files in:

- _$XDG_CONFIG_HOME/skeld_ (fallback _~/.config/skeld_)
- _$XDG_DATA_HOME/skeld_ (fallback _~/.local/share/skeld_)
- _$XDG_DATA_DIRS[0]/skeld_, _$XDG_DATA_DIRS[1]/skeld_,...
  (fallback _/usr/share/local/skeld_ and _/usr/share/skeld_)

These locations are referred to as _<SKELD-DATA>_.

# PROJECTS
Projects files are located in _<SKELD-DATA>/projects_. They must have the
extension 'toml' and the following content:

```
# NOTE: if not provided, the filename (without extension) is used
name = "nvim-config"
# NOTE: projects that don't specify a keybinding are assigned a number (starting
#       with 1) as the keybinding
keybind = "cv"

[project]
# see #PROJECT-DATA-FORMAT for supported options
```

# BOOKMARKS
Bookmark files are located in _<SKELD-DATA>/bookmarks_ and use the same format
as projects.

# PROJECT DATA FORMAT
This is the core format that describes everything that is needed to open a
project, including the configuration of the sandbox. The format used is TOML
with the following supported options:

```
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

# if 'whitelist-all-envvars' is true, all environment variables remain
# accessible; otherwise only the variables in 'whitelist-envvar' are transferred
# into the sandbox
whitelist-all-envvars = true # Default: false
whitelist-envvar = [ "..." ]

# include options from other files
# NOTE: circular includes are allowed
include = [
  # relative paths are searched in <SKELD-DATA>/include
  # (see #FILES)
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

## String Interpolation
Wherever a path is expected, the following placeholders can be used:

[- *Placeholder*
:- *Substition*
|- *$[*_ENVVAR_*]*
:- value of environment variable _ENVVAR_
|- *$[*_ENVVAR_*:*_ALTVAL_*]*
:- value of environment variable _ENVVAR_ if existent, otherwise _ALTVAL_
|- *$(CONFIG)*
:- *XDG_CONFIG_HOME* if existent, otherwise _~/.config_
|- *$(CACHE)*
:- *XDG_CACHE_HOME* if existent, otherwise _~/.cache_
|- *$(DATA)*
:- *XDG_DATA_HOME* if existent, otherwise _~/.local/share_
|- *$(STATE)*
:- *XDG_STATE_HOME* if existent, otherwise _~/.local/state_

# CONFIGURATION
The configuration is located at _$XDG_CONFIG_HOME/skeld/config.toml_ (fallback
_~/.config/skeld/config.toml_). The following options are supported:

```
# banner shown at the top
# NOTE: example was generated with figlet using the larry3d font
banner = '''
                               __
  ___      __    ___   __  __ /\\_\\    ___ ___
/' _ `\\  /'__`\\ / __`\\/\\ \\/\\ \\\\/\\ \\ /' __` __`\\
/\\ \\/\\ \\/\\  __//\\ \\_\\ \\ \\ \\_/ |\\ \\ \\/\\ \\/\\ \\/\\ \\
\\ \\_\\ \\_\\ \\____\\ \\____/\\ \\___/  \\ \\_\\ \\_\\ \\_\\ \\_\\
 \\/_/\\/_/\\/____/\\/___/  \\/__/    \\/_/\\/_/\\/_/\\/_/
'''
# disable the help text in the bottom right corner
disable-help = true

[colorscheme]
# colors can be specified as hex color codes
normal = "#DCD7BA"
# or as ansi color codes
# (see https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit)
banner = 3
heading = "#C0A36E"
label = "#727169"
keybind = "#6A9589"
background = "#1F1F28"

[[commands]]
name = "<edit>"
keybind = "e"
# if 'command' is empty, skeld quits immediatly
command = ["nvim", "--clean"]
# see 'detach' in #PROJECT-DATA-FORMAT
detach = false

# user-wide project data that is merged with per-project data
[project]
# see #PROJECT-DATA-FORMAT for supported options
```
