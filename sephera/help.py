import sys
try:
    from etc.generate.config_help import (
        MAIN_HELP, LOC_COMMAND_HELP, 
        STATS_COMMAND_HELP, TREE_COMMAND_HELP, 
        UPDATE_COMMAND_HELP, LANGUAGE_SUPPORT_COMMAND_HELP,
        VERSION_COMMAND_HELP, SET_CFG_COMMAND_HELP
    )
    from rich.console import Console
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class SepheraHelp:
    def __init__(self) -> None:
        self.console = Console()
    
    def usage(self) -> None:
        print(MAIN_HELP)

    def show_help(self, args: str) -> None:
        match args.lower().strip():
            case "loc":
                self.console.print(f"[cyan] {LOC_COMMAND_HELP}")
                sys.exit(0)
                
            case "stats":
                self.console.print(f"[cyan] {STATS_COMMAND_HELP}")
                sys.exit(0)

            case "tree":
                self.console.print(f"[cyan] {TREE_COMMAND_HELP}")
                sys.exit(0)

            case "update":
                self.console.print(f"[cyan]{UPDATE_COMMAND_HELP}")

            case "language-support":
                self.console.print(f"[cyan]{LANGUAGE_SUPPORT_COMMAND_HELP}")

            case "version":
                self.console.print(f"[cyan]{VERSION_COMMAND_HELP}")
            
            case "cfg-language":
                self.console.print(f"[cyan]{SET_CFG_COMMAND_HELP}")

            case _:
                self.console.print("\n".join([
                    f"[red][!] Invalid option: {args}",
                    "[yellow][+] Hint: Allowed options: [cyan]loc, stats, tree, update, language-support, version, cfg-language"
                ]))
                sys.exit(1)
