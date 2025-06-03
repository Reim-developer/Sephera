import os
import re
import sys
from typing import Optional

try:
    from rich.console import Console
    from datalyzer.Exporter import Exporter
    from rich.table import Table
    from utils.utils import Utils
    from utils.stdout import SepheraStdout
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class Stats:
    def __init__(self, base_path: str = ".", ignore_pattern: Optional[str] = None) -> None:
        self.base_path = base_path
        
        self.console = Console()
        self.stdout = SepheraStdout()
        self.ignore_regex: Optional[re.Pattern[str]] = None
        self.ignore_str: Optional[str] = None
        self.utils = Utils()

        if ignore_pattern:
            try:
                self.ignore_regex = re.compile(ignore_pattern)
            except re.error:
                self.ignore_str = ignore_pattern

    def _stdout_stats(self, data: dict[str, int | float]) -> None:
        total = sum(data.values())
        table = Table(title = "Sephera Stats Overview", show_header = True, header_style = "bold magenta")

        table.add_column("Category")
        table.add_column("Count", justify = "right")
        table.add_column("Percent", justify = "right")

        for key, value in data.items():
            percent = (value / total) * 100 if total else 0
            table.add_row(str(key), str(value), f"{percent:.1f}%")

        self.console.print(table)


    def stats_all_files(self, output_chart: str | None = None) -> None:
        file_count: int = 0
        folder_count: int = 0
        total_size: int = 0

        hidden_file_count: int = 0
        hidden_folder_count: int = 0
        total_hidden_size: int = 0

        with self.console.status("[bold green] Processing...", spinner = "material"):
            for root, dirs, files in os.walk(self.base_path):

                dirs[:] = [dir for dir in dirs if not 
                            self.utils.is_ignored(
                                path = os.path.join(root, dir), 
                                ignore_regex = self.ignore_regex, 
                                ignore_str = self.ignore_str)]

                for dir in dirs:
                        full_dir_path = os.path.join(root, dir)

                        if self.utils.is_hidden_path(path = full_dir_path, base_path = self.base_path):
                            hidden_folder_count += 1
                        folder_count += 1
                    
                for file in files:
                        file_count += 1
                        full_path = os.path.join(root, file)

                        try:
                            size = os.path.getsize(full_path)
                            total_size += size

                            if self.utils.is_hidden_path(full_path, self.base_path):
                                hidden_file_count += 1
                                total_hidden_size += size

                        except Exception as error:
                            self.stdout.die(error = error)

                self.console.clear()
        
        data: dict[str, int | float] = {
            "Folder": folder_count,
            "File": file_count,
            "Hidden Folder": hidden_folder_count,
            "Hidden File": hidden_file_count
        }
        
        self._stdout_stats(data = data)

        if output_chart is not None:
            exporter = Exporter(output_path = output_chart)

        print(f"[+] Total Size: {total_size / (1024 ** 2):.2f} MB")
        print(f"[+] Total Hidden Size: {total_hidden_size / (1024 ** 2):.2f} MB")

        if output_chart:
            exporter.export_stats_chart(data = data, total_size = total_size, total_hidden_size = total_hidden_size)
            print(f"[+] Saved chart as name: {output_chart}")
        
