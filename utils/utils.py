import re
import os
from typing import Optional, List
import fnmatch

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
            
            for ignore_query in ignore_str_set:
                if ignore_query == path_basename:
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