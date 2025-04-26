import argparse
import sys

try:
    from rich.console import Console
    from handler import Handler
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class Command:
    def __init__(self, sephera_parser: argparse.ArgumentParser) -> None:
        self.sephera_parser = sephera_parser

        self.console = Console()
        self.handler = Handler()
        self.sub_command = self.sephera_parser.add_subparsers(dest = "command")
    
    def setup(self) -> None:
        try:
            self._set_tree_command()
            self._set_stats_command()
            self._set_loc_command()
            self._set_help_command()
            self._set_update_command()
            self._set_languages_supports_command()
            self._set_version_command()
            self._set_language_cfg_command()
            
            args = self.sephera_parser.parse_args()
            if args.command is None:
                self.handler.show_usage(args = args)

        except Exception as setup_error:
             self.console.print(f"[red] Fatal error when setup command: {setup_error}")
             sys.exit(1)

    def _set_stats_command(self) -> None:
        stats_parser = self.sub_command.add_parser("stats", help = "Stats all files, folders in your directory")
        stats_parser.add_argument(
             "--path",
             type = str,
             help = "Path to scan.(Default is current directory)",
             default = "."
        )
        stats_parser.add_argument(
             "--ignore",
             type = str, 
             help = "Regex pattern to ignore files or folders (e.g --ignore '__pycache__|\\.git')",
             default = None
        )
        stats_parser.add_argument(
             "--chart",
             type = str,
             nargs = "?",
             const = "SepheraChart",
             help = "Create chart for your stat overview (e.g --chart '<MyChartSaveDir>')",
             default = None
        )
        stats_parser.set_defaults(function = self.handler.stats_command_handler)

    def _set_tree_command(self) -> None:
        tree_command = self.sub_command.add_parser("tree", help = "List tree view all files")
        tree_command.add_argument(
            "--path",
            type = str,
            help = "Path to scan (Default is current directory)",
            default = "."
        )
        tree_command.add_argument(
            "--ignore",
            type = str,
            help = "Regex pattern to ignore files or folders (e.g. --ignore '__pycache__|\\.git')",
            default = None
        )
        tree_command.add_argument(
            "--chart",
            type = str,
            nargs = "?",
            const = "SepheraChart",
            help = "Create chart for your directory tree (e.g --chart '<MyChartSaveDir>')",
            default = None
        )
        tree_command.set_defaults(function = self.handler.tree_command_handler)

    def _set_loc_command(self) -> None:
        loc_command = self.sub_command.add_parser("loc", help = "LOC your code in project. Quickly")
        loc_command.add_argument(
            "--path",
            type = str,
            help = "Path to your project.",
            default = "."
        )
        loc_command.add_argument(
            "--ignore",
            action = "append",
            type = str,
            help = "Regex pattern to ignore files or folders (e.g. --ignore '__pycache__|\\.git')",
            default = None
        )
        loc_command.add_argument(
            "--json",
            type = str,
            nargs = "?",
            const = "SepheraExport",
            help = "Export LOC count to JSON file format. e.g --json ('<MySaveDir/myJson>')",
            default = None
        )
        loc_command.add_argument(
            "--md",
            type = str,
            nargs = "?",
            const = "SepheraExport",
            help = "Export LOC count to Markdown file format. e.g --md ('<MySaveDir/myMarkdown>')",
            default = None
        )

        loc_command.set_defaults(function = self.handler.loc_command_handler)

    def _set_help_command(self) -> None:
        help_command = self.sub_command.add_parser("help", help = "Show help message")
        help_command.add_argument(
            "command",
            nargs = "*",
            help = "Display help about a command."
        )
        help_command.set_defaults(function = self.handler.help_command_handler)

    def _set_update_command(self) -> None:
        update_command = self.sub_command.add_parser("update", help = "Update Sephera if latest version is available")
        update_command.set_defaults(function = self.handler.update_command_handler)

    def _set_languages_supports_command(self) -> None:
        languages_support_command = self.sub_command.add_parser(
            "language-support", help = "Fetch languages Sephera can support for LOC count."
        )
        languages_support_command.add_argument(
            "--list",
            type = bool,
            const = True,
            nargs = "?",
            help = "Show list of languages currently supported by Sephera."
        )

        languages_support_command.set_defaults(function = self.handler.fetch_languages_supports_handler)

    def _set_version_command(self) -> None:
        version_command = self.sub_command.add_parser(
            "version", help = "Display Sephera version."
        )
        version_command.add_argument(
            "--git",
            type = bool,
            const = True,
            nargs = "?",
            help = "Show the Sephera latest version on GitHub."
        )
        version_command.set_defaults(function = self.handler.version_command_handler)

    def _set_language_cfg_command(self) -> None:
        language_cfg_command = self.sub_command.add_parser(
            "cfg-language"
        )
        language_cfg_command.add_argument(
            "--global",
            action = "store_true",
            dest = "global_",
            help = "Set the configuration for language detection and LOC count to global."
        )

        language_cfg_command.set_defaults(function = self.handler.language_cfg_handler)
    

