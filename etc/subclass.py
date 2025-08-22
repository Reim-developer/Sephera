import argparse
import sys
import re
from typing import NoReturn

try:
    from rich.console import Console
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class SepheraArgparse(argparse.ArgumentParser):
    def error(self, message: str) -> NoReturn:
        
        match = re.search(r"invalid choice: '(.+?)'", message)
        console = Console()
        if match:
            wrong_command = match.group(1)
            console.print(f"[red]Unrecognized command: '{wrong_command}'")
            console.print("[yellow]Hint: use 'sephera help' for more infomation")
            sys.exit(1)

        elif "expected one argument" in message:
            match = re.search(r"argument (.+?): expected one argument", message)
            if match:
                arg = match.group(1)
                console.print(f"[red]Missing value for argument: {arg}")
                sys.exit(1)

        
        console.print(f"[red]Unrecognized arugment: {message.replace('unrecognized arguments:', '').strip()}")
        sys.exit(1)
        
        