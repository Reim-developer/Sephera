import re
import os
from typing import Optional

class Utils:
    def is_ignored(self, path: str, ignore_regex: Optional[re.Pattern] = None, ignore_str: Optional[str] = None) -> None:
        if ignore_regex:
            return bool(ignore_regex.search(path))
        
        if ignore_str:
            return ignore_str in path
        
        return False
    
    def is_hidden_path(self, path: str, base_path: str) -> None:
        rel_path = os.path.relpath(path, base_path)
        parts = rel_path.split(os.sep)

        return any(part.startswith(".") for part in parts)