SKELD(1)

# NAME

skeld - a TUI tool for opening projects in a restricted sandbox

# SYNOPSIS

*skeld* [*--config* _config_file_] _subcommand_

# OPTIONS

*--config* _config_file_
	Override the config file to use. By default, the config file is located at
	_$XDG_CONFIG_HOME/skeld/config.toml_.

# SUBCOMMANDS

*ui*
	Launch the TUI.

*add* [*--name* _name_] _path_
	Add a project file for the project at _path_. Optionally, *--name* can be used
	to override the project name, which would otherwise be derived from the
	filename.

# DESCRIPTION

*skeld* is a TUI tool to open projects in a restricted sandbox. Each project is
defined by a TOML file that specifies which paths can be accessed within the
sandbox.

To open a project, use the TUI launched with *skeld ui*. Then, a project can be
opened using the Up/Down/J/K/Enter keys or the mouse.

# PROJECTS

Projects are defined by a TOML file located in one of the following locations:
- _$XDG_CONFIG_HOME/skeld/projects/_ (fallback _~/.config/skeld/projects/_)
- _$XDG_DATA_HOME/skeld/projects/_ (fallback _~/.local/share/skeld/projects/_)
- _$XDG_DATA_DIRS[0]/skeld/projects/_, _$XDG_DATA_DIRS[1]/skeld/projects/_, ...++
(fallback _/usr/share/skeld/projects/_ and _/usr/share/local/skeld/projects/_)

Each project file must be a valid TOML file with the ".toml" extension. The
following fields are supported:

*name*
	String.

	Name used to display the project in the TUI. If not provided, a name will be
	derived from the filename.

*keybind*
	String.

	Specifies a keybinding that can be used to open the project in the TUI. If not
	provided, an unused number will be used as the keybinding.

	Note that unlike keybindings defined in the user-wide configuration, key codes
	(such as "<CR>") are _not_ supported here.

*project*
	Table.

	The project data. Refer to *skeld-project-data*(5) for the available fields.

	Note that this will be merged with the user-wide data (see *skeld-config*(5)).

Bookmarks are also supported. They use the same file format, but must be placed
into the _bookmarks/_ directory instead of the _projects/_ directory.

# DEFAULT KEYBINDINGS

*Up*, *K*
	Select the button above.

*Down*, *J*
	Select the button below.

*Enter*
	Open the currently selected project.

*CONTROL-C*
	Exit the TUI.

For information on adding custom keybindings, refer to *skeld-config*(5).

# SEE ALSO

*skeld-config*(5), *skeld-project-data*(5)
