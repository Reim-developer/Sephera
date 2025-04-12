import argparse
import sys
import os

try:
    from rich.console import Console
    from chart.Exporter import Exporter
    from sephera.WalkFile import WalkFile
    from sephera.Stats import Stats
    from utils.error import SepheraError
    from rich.progress import Progress, SpinnerColumn, BarColumn, TextColumn, TimeElapsedColumn
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class SepheraCli:
    def __init__(self):
        self.sephera_parser = argparse.ArgumentParser(description = "Sephera Commmand Line Interface")
        sub_command = self.sephera_parser.add_subparsers(dest = "command", required = True)

        tree_command = sub_command.add_parser("tree", help = "List tree view all files")
        stats_command = sub_command.add_parser("stats", help = "Show stats of your file/project")

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
        tree_command.set_defaults(function = self.tree_command)

        stats_command.add_argument(
             "--path",
             type = str,
             help = "Path to scan.(Default is current directory)",
             default = "."
        )
        stats_command.add_argument(
             "--ignore",
             type = str, 
             help = "Regex pattern to ignore files or folders (e.g --ignore '__pycache__|\\.git')",
             default = None
        )
        stats_command.add_argument(
             "--chart",
             type = str,
             nargs = "?",
             const = "SepheraChart",
             help = "Create chart for your stat overview (e.g --chart '<MyChartSaveDir>')",
             default = None
        )
        stats_command.set_defaults(function = self.stats_command)

    def tree_command(self, args) -> None:
        console = Console()

        if not os.path.exists(args.path):
            error = SepheraError(console = console)
            error.show_error(f"Path: {args.path} not found.")

        walker = WalkFile(args.ignore, args.path)

        with Progress(
                    SpinnerColumn(), TextColumn("[progress.description]{task.description}"), 
                    TextColumn("[progress.description]"),
                    TimeElapsedColumn(), console = console, transient = True) as progress_bar:
                    task = progress_bar.add_task("Loading Tree...", total = None)
                    stats = walker.show_list_tree(on_step = lambda: progress_bar.update(task, advance = 1), console = console)

        if args.chart:
            with Progress(
                    SpinnerColumn(), TextColumn("[progress.description]{task.description}"), 
                    BarColumn(bar_width = 30), TextColumn("{task.completed}/{task.total}"),
                    TimeElapsedColumn(), console = console, transient = True) as progress_bar:
                    task = progress_bar.add_task("Exporting Chart...", total = 4)

                    exporter = Exporter(args.chart)
                    exporter.export_file_tree_chart(
                        files = stats["Files"],
                        dirs = stats["Directory"],
                        hidden_files = stats["Hidden_Files"],
                        hidden_dirs = stats["Hidden_Directory"],
                        on_step = lambda: progress_bar.update(task, advance = 1) 
                    )
            print(f"Successfully created chart with name: {args.chart}.png")

    def stats_command(self, args) -> None:
        console = Console()
        if not os.path.exists(args.path):
          error = SepheraError(console = console)
          error.show_error(f"Path: {args.path} not found.")
        
        stats = Stats(base_path = args.path, ignore_pattern = args.ignore)
        stats.stats_all_files(output_chart = args.chart)
       

if __name__ == "__main__":
    try:
        cli = SepheraCli()
        args = cli.sephera_parser.parse_args()
        args.function(args)
    except KeyboardInterrupt:
        print("\n Aborted by user.")
