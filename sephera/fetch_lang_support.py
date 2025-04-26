import sys

try:
    from rich.console import Console
    from utils.utils import Utils
    from __version__ import SEPHERA_VERSION
    
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class FetchLanguage:
    def __init__(self) -> None:
        self.console = Console()
        self.utils = Utils()
    
    def fetch_language_count(self, verbose: bool = False) -> None:
        if not verbose:
            languages = self.utils.fetch_support_languages()
            self.console.print("\n".join([
                f"[cyan][+] Total language(s) supported by Sephera: {languages}",
                f"[cyan][+] Sephera current version: {SEPHERA_VERSION}"
            ]))
        
        else:
            language_names = self.utils.fetch_support_languages_name()
            language_count: int = 0

            for language in language_names:
                language_count += 1
                self.console.print(f"[cyan][{language_count}] {language}")
        

        