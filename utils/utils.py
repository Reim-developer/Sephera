import re
import os
import fnmatch
import requests
import sys
from typing import Optional, List
from rich.console import Console
from __version__ import SEPHERA_VERSION
from packaging import version

class Utils:
    def is_ignored(self, path: str, ignore_regex: Optional[re.Pattern] = None, ignore_str: Optional[str] = None) -> bool:
        if ignore_regex:
            return bool(ignore_regex.search(path))
        
        if ignore_str:
            return ignore_str in path
        
        return False
    
    def is_multi_ignored(
            self, path: str, ignore_regex: Optional[List[re.Pattern]] = None, 
            ignore_str: Optional[List[str]] = None,
            ignore_glob: Optional[List[str]] = None
        ) -> bool:
        if ignore_regex:
            for regex in ignore_regex:
                if regex.search(path):
                    return True
        
        if ignore_str:
            path_basename = os.path.basename(path)
            ignore_str_set = set(ignore_str)
            
            if path_basename in ignore_str_set:
                return True
        
        if ignore_glob:
            path_basename = os.path.basename(path)
    
            for glob in ignore_glob:
                if fnmatch.fnmatch(path_basename, glob):
                    return True

        return False 
    
    def is_hidden_path(self, path: str, base_path: str) -> bool:
        rel_path = os.path.relpath(path, base_path)
        parts = rel_path.split(os.sep)

        return any(part.startswith(".") for part in parts)
    
    def is_path_exists(self, path: str) -> bool:
        return os.path.exists(path = path)

    def fetch_latest_version(self) -> str:
        console = Console()

        GITHUB_REPO = "Reim-developer/Sephera"
        GITHUB_API = f"https://api.github.com/repos/{GITHUB_REPO}/releases/latest"

        request_headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36"
        }

        try:
            request = requests.get(url = GITHUB_API, headers = request_headers)

        except Exception as error:
            console.print("\n".join([
                "[red][+] Error when fetch latest verion of Sephera:",
                f"[red][+] Error name: {type(error).__name__}",
                f"[red][+] Error details: [yellow]{error}"
            ]))
            sys.exit(1)

        data = request.json()
        
        version_tag: str = data.get("tag_name", "")

        return version_tag.lstrip("v")
    
    def is_latest_version(self) -> bool:
        latest_version = version.parse(self.fetch_latest_version())
        current_version = version.parse(SEPHERA_VERSION)

        if latest_version > current_version or latest_version == current_version:
            return True
    
        else:
            return False



        