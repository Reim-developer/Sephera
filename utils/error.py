import sys

try:
    from rich.console import Console
    from rich.panel import Panel
    from rich.text import Text
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class SepheraError:
    def __init__(self) -> None:
        self.console = Console()

    def show_error(self, message: str) -> None:
        panel = Panel.fit(
            Text(message, style = "bold red"),
            title = "[bold white on red] ERROR [/]",
            border_style = "red"
        )
        self.console.print(panel)
        sys.exit(1)
