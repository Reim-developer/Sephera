import sys

try:
    from rich.console import Console
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class ConfirmInteractive:
    def __init__(self) -> None:
        self.console = Console()
        self.RE_INSTALL_CONFIRM: str = "re_install"
        self.INSTALL_TO_ANOTHER_PATH: str = "install_to_another_path"
        self.EXIT_CONFIRM: str = "confirm_exit"
    
    def verbose_confirm(self) -> bool:
        try:
            self.console.print("\n".join([
                "[cyan][+] Your task is successfully. Do you want:",
                "[yellow][1] [cyan]Show me verbose infomation.",
                "[yellow][2] [cyan]No, just show me short-infomation.",
                "[yellow][!] Default as 2 if you leave blank."
            ]))
            option: str = input("Your option [1, 2 ]: ").strip()

            if not option:
                return False
            
            match option:
                case "1": 
                    return True
                
                case "2": 
                    return False
                
                case _: 
                    return False

        except KeyboardInterrupt:
            return False
        
    def latest_version_option(self) -> str:
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
                    return self.RE_INSTALL_CONFIRM

                case "2":
                    return self.INSTALL_TO_ANOTHER_PATH

                case "3":
                    return self.EXIT_CONFIRM

                case _:
                    self.console.print(f"[red]Invalid option: {prompt_value}. Type '3' to exit.")

    def override_write_confirm(self, file_name: str) -> bool:
        try:
            while True:
                self.console.print("\n".join([
                    f"[yellow][!] Your data/config {file_name} is already exists. Do you want",
                    f"[cyan][1] Override all data/config in {file_name}.",
                    "[cyan][2] No override. Cancel write data/config, and exit now.",
                    "[yellow][!] Default as 1 if you leave blank."
                ]))

                option: str = input("Your option [1, 2]: ").strip()

                if not option:
                    return True
                
                match option:
                    case "1": 
                        return True
                    
                    case "2": 
                        return False

                    case _:
                        self.console.print(f"[red] Invalid option: {option}")

        except KeyboardInterrupt:
            self.console.print("\n[cyan][+] Aborted by user.")
            return False
                