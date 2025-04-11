import argparse
from walk.WalkFile import WalkFile

class Sephera:
    def __init__(self):
        self.sephera_parser = argparse.ArgumentParser(description = "Sephera Commmand Line Interface")
        sub_command = self.sephera_parser.add_subparsers(dest = "command", required = True)

        tree_command = sub_command.add_parser("tree", help = "List tree view all files")
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
        tree_command.set_defaults(function = self.walk_files)

    def walk_files(self, args) -> None:
        walker = WalkFile(args.ignore)
        stats = walker.show_list_tree()

        if args.chart:
            from chart.Exporter import Exporter
            from rich.console import Console
            from rich.progress import Progress, SpinnerColumn, BarColumn, TextColumn, TimeElapsedColumn

            console = Console()
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

if __name__ == "__main__":
    cli = Sephera()
    args = cli.sephera_parser.parse_args()
    args.function(args)
