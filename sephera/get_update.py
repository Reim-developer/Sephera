import sys

try:
    from utils.utils import Utils
    from rich.console import Console
    from sephera.interactive.confirm import ConfirmInteractive
    from sephera.interactive.option import OptionHandler
    from sephera.net.network_helper import NetworkHelper
    from utils.stdout import SepheraStdout
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class GetUpdate:
    def __init__(self) -> None:
        self.utils = Utils()
        self.console = Console()
        self.confirm_interactive = ConfirmInteractive()
        self.network = NetworkHelper()
        self.option_interactive = OptionHandler()
                    
    def update_sephera(self) -> None:
        try:
            is_latest_version: bool = self.utils.is_latest_version(console = self.console)

            if is_latest_version:
                    user_option = self.confirm_interactive.latest_version_option()

                    match user_option:
                        case self.confirm_interactive.RE_INSTALL_CONFIRM:
                            self.network.install_sephera()

                        case self.confirm_interactive.INSTALL_TO_ANOTHER_PATH:
                            dir_path = self.option_interactive.on_choose_dir_path()

                            if dir_path: 
                                self.network.install_sephera(save_dir= dir_path)

                        case self.confirm_interactive.EXIT_CONFIRM:
                            sys.exit(0)

                        case _:
                            pass # Ignore.
                            
            else:
                self.network.install_sephera()
            
        except KeyboardInterrupt:
            self.console.print("\n[cyan][+] Aborted by user.")

        except Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)            
