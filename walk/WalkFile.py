import os
import re
from typing import Generator, Optional, Callable

try:
    from rich.console import Console
except KeyboardInterrupt:
    print("\n Aborted by user.")

from utils.error import SepheraError

"""" 
Sephera Command Line Interface
WalkFile class

"""
class WalkFile:
    def __init__(self, ignore_pattern: Optional[str] = None, base_path: str = ".") -> None:
        self.base_path = base_path
        self.ignore_regex: Optional[re.Pattern] = None
        self.ignore_str: Optional[str] = None
        
        if ignore_pattern:
            try:
                self.ignore_regex = re.compile(ignore_pattern)
            except re.error:
                self.ignore_regex = None
                self.ignore_str = ignore_pattern

    def _is_ignored(self, path: str) -> bool:
        if self.ignore_regex:
            return bool(self.ignore_regex.search(path))
        
        if self.ignore_str:
            return self.ignore_str in path
        
        return False

    def walk_all_files(self) -> Generator[str, None, None]:
        for root, dirs, files in os.walk(self.base_path):
            dirs[:] = [dir for dir in dirs if not self._is_ignored(os.path.join(root, dir))]

            for file in files:
                file_path = os.path.join(root, file)

                if self._is_ignored(file_path):
                    continue

                yield os.path.join(root, file)

    def show_list_tree(self, on_step: Callable[[], None] = None, console: Console = None) -> dict[str, int]:
        folder_count: int = 0
        file_count: int = 0
        hidden_file_count: int = 0
        hidden_folder_count: int = 0
        output: list[str] = []

        for root, dirs, files in os.walk(self.base_path):
            on_step()
            for dir in list(dirs):
                full_path = os.path.join(root, dir)

                if self._is_ignored(full_path):
                    dirs.remove(dir)
                    continue
                
                if dir.startswith("."):
                    hidden_folder_count += 1
                on_step()
                
            for file in list(files):
                full_path = os.path.join(root, file)

                if self._is_ignored(full_path):
                    continue

                if file.startswith("."):
                    hidden_file_count += 1
                else:
                    file_count += 1
                on_step()

            folder_count += len(dirs)

        self._show_list_tree(self.base_path, prefix = "", console = console, output = output)
        for line in output:
            print(f"{line}")

        print(f"{folder_count} Folder. {file_count} File.")

        return {
            "Files": file_count,
            "Directory": folder_count,
            "Hidden_Files": hidden_file_count,
            "Hidden_Directory": hidden_folder_count
        }

    def _show_list_tree(self, current_dir: str, prefix: str, console: Console, output: list[str]) -> None:
        try:
            entries = sorted(os.listdir(current_dir))
        except PermissionError:
            error = SepheraError(console = console)
            error.show_error(f"Permission Denied. Skipping: {current_dir}")
            return
        
        entries = [e for e in entries if not e.startswith(".")]
        for i, entry in enumerate(entries):
            full_path = os.path.join(current_dir, entry)

            if self.ignore_regex and self.ignore_regex.search(full_path):
                continue

            connector = "└── " if i == len(entries) - 1 else "├── "
            output.append(f"{prefix}{connector}{entry}")

            if os.path.isdir(full_path):
                extension = "    " if i == len(entries) - 1 else "│   "
                self._show_list_tree(full_path, prefix + extension, console = console, output = output)
