import os
import re
from typing import Generator, Optional, Union

"""" 
Sephera Command Line Interface
WalkFile class

"""
class WalkFile:
    def __init__(self, ignore_pattern: Optional[str] = None) -> None:
        self.base_dir = "."
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
        for root, dirs, files in os.walk(self.base_dir):
            dirs[:] = [dir for dir in dirs if not self._is_ignored(os.path.join(root, dir))]

            for file in files:
                file_path = os.path.join(root, file)

                if self._is_ignored(file_path):
                    continue

                yield os.path.join(root, file)

    def show_list_tree(self) -> None:
        folder_count: int = 0
        file_count: int = 0
        for root, dirs, files in os.walk(self.base_dir):
            dirs[:] = [dir for dir in dirs if not self._is_ignored(os.path.join(root, dir))]
            files = [file for file in files if not self._is_ignored(os.path.join(root, file))]

            folder_count += len(dirs)
            file_count += len(files)

        self._show_list_tree(self.base_dir, prefix = "")
        print(f"{folder_count} Folder. {file_count} File.")

    def _show_list_tree(self, current_dir: str, prefix: str) -> None:
        try:
            entries = sorted(os.listdir(current_dir))
        except PermissionError:
            print("Permission Error, Sephera is not permission to run this command")
        
        entries = [e for e in entries if not e.startswith(".")]
        for i, entry in enumerate(entries):
            full_path = os.path.join(current_dir, entry)

            if self.ignore_regex and self.ignore_regex.search(full_path):
                continue

            connector = "└── " if i == len(entries) - 1 else "├── "
            print(f"{prefix}{connector}{entry}")

            if os.path.isdir(full_path):
                extension = "    " if i == len(entries) - 1 else "│   "
                self._show_list_tree(full_path, prefix + extension)
                
       