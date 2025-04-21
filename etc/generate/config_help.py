# ================================================================

# Auto-generated file config from YAML configuration
# You can customize this config via config/languages.yml file
# If this file is not exists, you can find this in:
# https://github.com/Reim-developer/Sephera/tree/master/config
# This project is licensed under the GNU General Public License v3.0
# https://github.com/Reim-developer/Sephera?tab=GPL-3.0-1-ov-file

# ==============================================================

MAIN_HELP = """Usage: sephera [options...]

    loc                        Quickly calculate total lines of code in your project.
    stats                      Show your project stats, size, number of files and folders. Optional chart output.
    tree                       Show your project structure tree. Optional chart output.

Use `sephera --help [command]` for more information."""

LOC_COMMAND_HELP = """Usage: sephera loc [arguments...]

Arguments:
    --path <path>              Path to your project. (Default: current directory)
    --ignore <pattern>         Regex pattern to ignore files or folders. Can be used multiple times."""

STATS_COMMAND_HELP = """Usage: sephera stats [arguments...]

Arguments:
    --path <path>              Path to scan. (Default: current directory)
    --ignore <pattern>         Regex, glob, or name pattern to ignore files or folders (e.g. --ignore '__pycache__')
    --chart [<save_path>]      Create chart for your stat overview. Default chart name is 'SepheraChart'."""

TREE_COMMAND_HELP = """Usage: sephera tree [arguments...]

Arguments:
    --path <path>              Path to scan. (Default: current directory)
    --ignore <pattern>         Regex, or name pattern to ignore files or folders (e.g. --ignore '__pycache__')
    --chart [<save_path>]      Create chart for your directory tree. Default chart name is 'SepheraChart'."""
