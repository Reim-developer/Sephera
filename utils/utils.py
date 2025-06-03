import re
import os
import fnmatch
import requests
import sys
import platform
from sqlite3 import Cursor, Connection
from typing import Optional, List

try:
    from rich.console import Console
    from __version__ import SEPHERA_VERSION
    from packaging import version
    from .stdout import SepheraStdout
    from etc.generate.config_data import CONFIG_DATA
    from datalyzer.sql import SqlManager
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class Utils:
    def __init__(self) -> None:
        self.LINUX_PLATFORM: int = 1
        self.MACOS_PLATFORM: int = 2
        self.WINDOWS_PLATFORM: int = 3
        self.UNKNOWN_PLATFORM: int = 4

        self.GITHUB_REPO = "Reim-developer/Sephera"
        self.stdout = SepheraStdout()
        self.cursor: Optional[Cursor] = None
        self.connection: Optional[Connection] = None

    def is_ignored(self, path: str, ignore_regex: Optional[re.Pattern[str]] = None, ignore_str: Optional[str] = None) -> bool:
        if ignore_regex:
            return bool(ignore_regex.search(path))
        
        if ignore_str:
            return ignore_str in path
        
        return False
    
    def is_multi_ignored(
            self, path: str, ignore_regex: Optional[List[re.Pattern[str]]] = None, 
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

    def fetch_latest_version(self, console: Console) -> str:
        request_headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36"
        }

        GITHUB_API = f"https://api.github.com/repos/{self.GITHUB_REPO}/releases/latest"
        try:
            request = requests.get(url = GITHUB_API, headers = request_headers)
            request.raise_for_status()

        except Exception as error:
            console.clear()
            self.stdout.die(error = error)
            sys.exit(1)

        data = request.json()

        version_tag: str = data.get("tag_name", "")
        return version_tag.lstrip("v")
    
    def is_latest_version(self, console: Console) -> bool:
        latest_version = self.fetch_latest_version(console = console)
        current_version = version.parse(SEPHERA_VERSION)

        if latest_version == '':
            console.print("\n".join([
                "[red][!] Could not get latest version of Sephera, please try again later."
            ]))
            sys.exit(1)

        latest_version = version.parse(latest_version)
        if latest_version > current_version or latest_version == current_version:
            return True
    
        else:
            return False

    def os_detect(self) -> int:
        match platform.system():
            case "Windows": 
                return self.WINDOWS_PLATFORM
            
            case "Linux": 
                return self.LINUX_PLATFORM
            
            case "Darkwin": 
                return self.MACOS_PLATFORM
            
            case _: 
                return self.UNKNOWN_PLATFORM

    def fetch_support_languages(self) -> int:
        languages = len(CONFIG_DATA.get("languages", "name"))

        return languages
    
    def fetch_support_languages_name(self) -> list[str]:
        languages = CONFIG_DATA.get("languages", [])

        languages_names = [language["name"] for language in languages]

        return languages_names
    
    def get_local_data(self) -> str | int:
        user_home = os.path.expanduser("~")

        match self.os_detect():
            case self.WINDOWS_PLATFORM:
                return os.path.join(user_home, "AppData", "Local")
            
            case self.LINUX_PLATFORM | self.MACOS_PLATFORM:
                return user_home
            
            case _:
                return self.UNKNOWN_PLATFORM

    def sql_execute(self, sql: SqlManager, query: str, params: tuple[str | None, ...] = ()) -> None:
        try:
            if sql.cursor is not None:
                sql.cursor.execute(query, params)

                if sql.connection is not None:
                    sql.connection.commit()
        
        except Exception as error:
            self.stdout.die(error = error)
            sys.exit(1)
            