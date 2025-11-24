SKELD-PROJECT-DATA(5)

# NAME

skeld - project data options

# DESCRIPTION

These are the options used to define a project. This includes the project
directory, sandbox settings and editor to use.

Refer to *skeld*(1) or *skeld-config*(5) on where these options are used.

# OPTIONS

*project-dir*
	String.

	Root directory of the project.

*initial-file*
	String.

	File to open initially.

*no-sandbox*
	Boolean.

	Disable the sandbox entirely.

	Default: false

*whitelist-ro*
	Array of Strings.

	List of paths to whitelist with read-only access.

*whitelist-rw*
	Array of Strings.

	List of paths to whitelist with read-write access.

*whitelist-dev*
	Array of Strings.

	List of paths to whitelist with read-write access and allowing device access.
	Must be used to whitelist devices files.

*whitelist-ln*
	Array of Strings.

	List of symlinks to copy into the sandbox. Note that this does _not_ whitelist
	the target of the symlink.

*add-tmpfs*
	Array of Strings.

	Mounts a tmpfs inside the sandbox at the specified paths.

*whitelist-envvar*
	Array of Strings.

	List of environment variables to transfer into the sandbox.

*whitelist-all-envvars*
	Boolean.

	If true, all environment variables are transferred into the sandbox.
	Otherwise, only the variables from *whitelist-envvar* are transferred.

	Default: false

*include*
	Array of Strings.

	Merge the config options from the specified files.

	Options that can be merged (such as whitelists) will be merged. If an
	unmergeable option is defined more than once, an error will be raised, unless
	one of the definitions has a higher priority (see *PRIORITY SYSTEM*).

	Absolute paths are taken as-is, whereas relative paths are searched in the
	following locations:
	- _$XDG_CONFIG_HOME/skeld/include/_ (fallback _~/.config/skeld/include/_)
	- _$XDG_DATA_HOME/skeld/include/_ (fallback _~/.local/share/skeld/include/_)
	- _$XDG_DATA_DIRS[0]/skeld/include/_, _$XDG_DATA_DIRS[1]/skeld/include/_, ...++
(fallback _/usr/share/skeld/include/_ and _/usr/share/local/skeld/include/_)

*editor*
	Table.

	Specifies the editor/IDE used to open the project. Must contain the following
	fields:

	- *cmd*: An array containing the program and its arguments.

		In addition to standard string interpolation, *$(FILE)* can also be used. If
		an initial file exists, *$(FILE)* is replaced with its path. Otherwise, any
		argument containing *$(FILE)* is removed. Alternatively, use
		*(FILE:*_FALLBACK_*)* to replace with _FALLBACK_ when no initial file is
		provided.

	- *detach*: Whether to detach the program from the controlling terminal.

		If false, the editor takes over the terminal currently running skeld. If
		true, the editor is detached and skeld terminates afterwards. This should
		typically be true for GUI editors and false for TUI editors.

*defaults*
	Table.

	Used to specify a different priority level. See *PRIORITY SYSTEM*.

*forced*
	Table.

	Used to specify a different priority level. See *PRIORITY SYSTEM*.

# STRING INTERPOLATION

The following placeholders can be used in any path:

[- *Placeholder*
:- *Substitution*
|- *$[*_ENVVAR_*]*
:- Value of the environment variable _ENVVAR_
|- *$[*_ENVVAR_*:*_ALTVAL_*]*
:- Value of the environment variable _ENVVAR_ if existent, otherwise _ALTVAL_
|- *$(CONFIG)*
:- *$XDG_CONFIG_HOME* if existent, otherwise _~/.config_
|- *$(CACHE)*
:- *$XDG_CACHE_HOME* if existent, otherwise _~/.cache_
|- *$(DATA)*
:- *$XDG_DATA_HOME* if existent, otherwise _~/.local/share_
|- *$(STATE)*
:- *$XDG_STATE_HOME* if existent, otherwise _~/.local/state_

# PRIORITY SYSTEM

Project options are combined from various locations (user-wide configuration,
included files). Some options are merged together (such as whitelists), while
other options can only be set once (such as *editor*). The priority system can
be used to override an unmergable option set in a different location. For this
purpose there are three priority levels: *defaults*, *normal* and *forced*.

Unless otherwise specified, the *normal* priority is used. Other priorities can
be selected using the *defaults* and *forced* fields. The options defined within
these tables are applied with the corresponding priority.

# SEE ALSO

*skeld*(1), *skeld-config*(5)
