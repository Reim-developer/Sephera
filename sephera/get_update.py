import sys

try:
    from utils.utils import Utils
    from rich.console import Console
except KeyboardInterrupt:
    print("\n Aborted by user.")

class GetUpdate:
    def __init__(self) -> None:
        self.utils = Utils()
        self.console = Console()
        
    def _latest_version_option(self) -> None:
        while True:
            self.console.print("\n".join([
                "[yellow][!] You're using latest version of Sephera, do you want:",
                "[cyan][1] Re-install Sephera.",
                "[cyan][2] Install to another directory path.",
                "[cyan][3] Cancel and exit now."
            ]))
            prompt_value: str = input("Your option [1-3]: ").strip()

            match prompt_value:
                case "1":
                    # Todo: Re-install Sephera logic here
                    pass

                case "2":
                    # Todo: Re-install Sephera in another directory path here
                    pass

                case "3": sys.exit(0)

                case _:
                    self.console.print(f"[red]Invalid option: {prompt_value}. Type '3' to exit.")
                    
    def update_sephera(self) -> None:
        is_latest_version: bool = self.utils.is_latest_version()

        if is_latest_version:
            try:
                self._latest_version_option()
            
            except KeyboardInterrupt:
                self.console.print("\n[cyan][+] Aborted by user.")
