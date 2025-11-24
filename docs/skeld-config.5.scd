SKELD-CONFIG(5)

# NAME

skeld - user-wide configuration

# DESCRIPTION

The user-wide configuration file of skeld is located at
_$XDG_CONFIG_HOME/skeld/config.toml_ (fallback _~/.config/skeld/config.toml_).
Another location can be used by providing the *--config* command-line flag.

For project-specific configuration, refer to *skeld*(1).

# OPTIONS

The configuration file must be a valid TOML file. The following fields are
supported:

*banner*
	String.

	The banner displayed at the top of the TUI.

*colorscheme*
	Table.

	The following fields can be used to set the respective color:
	- *normal*
	- *banner*
	- *heading*
	- *keybind*
	- *project-name*
	- *background*

	Colors can be defined as either:
	- A hexadecimal color string ("#RRGGBB")
	- An ANSI color code (see:
	  https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit).

*project-button-width*
	Integer.

	Width of the project buttons in the TUI.

	Default: 40

*disable-default-keybinds*
	Boolean.

	Whether to disable the default keybindings (see *skeld*(1) for the default
	keybindings).

	Default: false

*keybinds*
	Array of Tables.

	An array of keybindings, where each keybinding is defined as a table with the
	following fields:

	*keys*
		A string defining the key sequence (in Vim-like format).

		Specifically, the following are recognized:
		- *<Nul>*, *<BS>*, *<Tab>*, *<Esc>*, *<Space>*, *<Del>*
		- *<CR>*, *<Return>*, *<Enter>*
		- *<Up>*, *<Down>*, *<Left>*, *<Right>*
		- *<F1>*, ..., *<F12>*
		- *<Insert>*, *<Home>*, *<End>*, *<Pageup>*, *<PageDown>*

		Modifiers can be specified by using:
		- *<C-*...*>* for Control
		- *<S-*...*>* for Shift
		- *<A-*...*>* for Alt
		- *<M-*...*>* for Meta
		- *<D-*...*>* for Super

		Literal "<" can be represented with *<lt>*. When there is no confusion
		possible, "<" can be used directly.

		Note that support for certain key combinations may depend on the host
		terminal.

	*action*:
		Defines the action of the keybinding. Can either be one of the following
		strings:
		- *move_up*: Select the button above.
		- *move_down*: Select the button below.
		- *choose*: Open the project of the currently selected button.
		- *quit*: Exit the TUI.
		- *nop*: Do nothing (useful for removing default keybindings).

		Alternatively, an external program can be launched, specified by a table
		with the following fields:
		- *cmd*: An array containing the program and its arguments.
		- *detach*: A boolean indicating whether to detach the program from the
		  terminal.

*project*
	Table.

	Defines the user-wide project data that will be merged with the
	project-specific data. Refer to *skeld-project-data*(5) for the available
	fields.

# SEE ALSO

*skeld*(1), *skeld-project-data*(5)
