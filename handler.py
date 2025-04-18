import sys
import os

try:
    from rich.console import Console
    from utils.stdout import SepheraStdout
    from sephera.Stats import Stats
    from sephera.WalkFile import WalkFile
    from chart.Exporter import Exporter
except KeyboardInterrupt:
    print(f"\n Aborted by user.")
    sys.exit(1)

class Handler:
    def __init__(self) -> None:
        self.console = Console()
        self.sephera_error = SepheraStdout()

    def stats_command_handler(self, args) -> None:
        if not os.path.exists(args.path):
            self.sephera_error.show_error(f"Fatal error: {args.path} not found.")

        stats = Stats(base_path = args.path, ignore_pattern = args.ignore)
        stats.stats_all_files(output_chart = args.chart)

    def tree_command_handler(self, args) -> None:

        if not os.path.exists(args.path):
            error = SepheraStdout(console = self.console)
            error.show_error(f"Path: {args.path} not found.")

        walker = WalkFile(args.ignore, args.path)
       
        stats = walker.show_list_tree()

        if args.chart:
            exporter = Exporter(args.chart)
            exporter.export_file_tree_chart(
                    files = stats["Files"],
                    dirs = stats["Directory"],
                    hidden_files = stats["Hidden_Files"],
                    hidden_dirs = stats["Hidden_Directory"],
                        
            )
            self.console.print(f"[cyan][+] Successfully created chart with name: {args.chart}.png")
    
