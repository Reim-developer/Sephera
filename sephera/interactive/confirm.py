import sys

try:
    from rich.console import Console
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class ConfirmInteractive:
    def __init__(self) -> None:
        self.console = Console()
    
    def verbose_confirm(self) -> bool:
        try:
            self.console.print("\n".join([
                "[cyan][+] Your task is successfully. Do you want:",
                "[yellow][1] [cyan]Show me verbose infomation.",
                "[yellow][2] [cyan]No, just show me short-infomation.",
                "[yellow][!] Default as 2 if you leave blank."
            ]))
            option: str = input("Your option [1, 2, 3]: ").strip()

            if not option:
                return False
            
            match option:
                case "1": return True
                case "2": return False
                case _: return False

        except KeyboardInterrupt:
            return False            