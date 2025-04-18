import argparse
import sys
import os

try:
    from rich.console import Console
    from chart.Exporter import Exporter
    from sephera.WalkFile import WalkFile
    from sephera.Stats import Stats
    from utils.stdout import SepheraStdout
    from handler import Handler
    from rich.progress import Progress, SpinnerColumn, BarColumn, TextColumn, TimeElapsedColumn
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class Command:
    def __init__(self, sephera_parser: argparse.ArgumentParser) -> None:
        self.sephera_parser = sephera_parser

        self.console = Console()
        self.handler = Handler()
        self.sub_command = self.sephera_parser.add_subparsers(dest = "command", required = True)

    def setup(self) -> None:
        try:
            self._set_tree_command(self.sub_command)
            self._set_stats_command()

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

    def _set_tree_command(self, tree_command: argparse.ArgumentParser) -> None:
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

