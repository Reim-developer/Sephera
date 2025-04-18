import sys
import os

try:
    from rich.console import Console
    from utils.error import SepheraError
    from sephera.Stats import Stats
except KeyboardInterrupt:
    print(f"\n Aborted by user.")
    sys.exit(1)

class Handler:
    def __init__(self) -> None:
        self.console = Console
        self.sephera_error = SepheraError()

    def stats_command_handler(self, args) -> None:
        if not os.path.exists(args.path):
            self.sephera_error.show_error(f"Fatal error: {args.path} not found.")

        stats = Stats(base_path = args.path, ignore_pattern = args.ignore)
        stats.stats_all_files(output_chart = args.chart)
