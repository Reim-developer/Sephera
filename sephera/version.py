import sys
from packaging import version

try:
    from rich.console import Console
    from __version__ import SEPHERA_VERSION
    from utils.utils import Utils
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class VersionCommand:
    def __init__(self) -> None:
        self.console = Console()
        self.utils = Utils()

    def fetch_version(self, show_git_version: bool) -> None:
        if not show_git_version:
            self.console.print("\n".join([
                f"[cyan][+] Sephera current version: {SEPHERA_VERSION}",
                "[cyan][+] If you want to fetch the latest version of Sephera on GitHub, use the '--git' flag."
            ]))

        else:
            with self.console.status("Fetching latest version...", spinner = "material"):
                sephera_version = self.utils.fetch_latest_version(console = self.console)
            
            sephera_version = version.parse(sephera_version)

            self.console.clear()
            self.console.print("\n".join([
                f"[cyan][+] The Sephera latest version on GitHub: {sephera_version}",
                f"[cyan][+] Your Sephera version: {SEPHERA_VERSION}",
                f"[cyan][+] You are {"use the latest version of Sephera" 
                                           if version.parse(SEPHERA_VERSION) == sephera_version 
                                           else "not use the latest version of Sephera"}" 
            ]))
        
