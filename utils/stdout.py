import sys

try:
    from rich.console import Console
    from rich.panel import Panel
    from rich.text import Text
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class SepheraStdout:
    def __init__(self) -> None:
        self.console = Console()

    def show_error(self, message: str) -> None:
        panel = Panel.fit(
            Text(message, style = "bold red"),
            title = "Error",
            border_style = "red"
        )
        self.console.print(panel)

    def die(self, error: Exception) -> None:
        self.console.print("\n".join([
            "[red][+] Error when use command:",
            f"[red][+] Error name: {type(error).__name__}",
            f"[red][+] Error details: [yellow]{error}"
        ]))
        
    def show_msg(self, message: str) -> None:
        panel = Panel.fit(
            Text(text = message, style = "bold cyan"),
            title = "Infomation",
            border_style = "cyan"
        )
        self.console.print(panel)
