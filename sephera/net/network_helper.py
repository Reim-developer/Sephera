import sys
import os
import time 

try:
    import requests
    from rich.console import Console
    from utils.utils import Utils
    from rich.progress import Progress, BarColumn, DownloadColumn, TimeRemainingColumn, TransferSpeedColumn

except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class NetworkHelper:
    def __init__(self) -> None:
        self.console = Console()
        self.utils = Utils()

    def install_sephera(self, save_dir: str = None) -> None:
        os_type: int = self.utils.os_detect()

        os_binary = {
            self.utils.LINUX_PLATFORM: "sephera_linux",
            self.utils.MACOS_PLATFORM: "sephera_macos",
            self.utils.WINDOWS_PLATFORM: "sephera.exe"
        }

        sephera_binary = os_binary.get(os_type)

        with self.console.status("Fetching latest version of Sephera...", spinner = "material"):
            latest_version = self.utils.fetch_latest_version()

        if not sephera_binary:
            self.console.print("[red][+]Unsupported operating system. Could not update Sephera.")
            sys.exit(1)
        
        if save_dir and not os.path.exists(save_dir):
            self.console.print(f"[red][!] Directory path: {save_dir} not found.")
            sys.exit(1)

        if save_dir is None:
            save_dir = os.getcwd()

        current_binary_path = os.path.realpath(sys.argv[0])
        DOWNLOAD_URL: str = f"https://github.com/{self.utils.GITHUB_REPO}/releases/download/v{latest_version}/{sephera_binary}"

        try:
            with Progress(
                "[progress.description]{task.description}", BarColumn(),
                DownloadColumn(), TransferSpeedColumn(), TimeRemainingColumn(),
                console = self.console,
                transient = True
                ) as progressBar:

                response = requests.get(DOWNLOAD_URL, stream = True)
                install_path = os.path.join(save_dir, sephera_binary)

                binary_size = int(response.headers.get("content-length", 0))
                task = progressBar.add_task(f"[cyan]Downloading: {sephera_binary}", total = binary_size)

                start_time: float = time.perf_counter()
                with open(file = install_path, mode = "wb") as sephera:
                    for chunk in response.iter_content(chunk_size = 8192):
                        if chunk: 
                            sephera.write(chunk)
                            progressBar.update(task, advance = len(chunk))
                
                end_time: float = time.perf_counter()

                if os_type in [self.utils.LINUX_PLATFORM, self.utils.MACOS_PLATFORM]:
                    os.chmod(install_path, 0o755)

                if os.path.realpath(save_dir) == os.path.dirname(current_binary_path):
                    os.replace(install_path, current_binary_path)
                    self.console.print("\n".join([
                        f"[cyan][+] Install {current_binary_path} successfully.",
                        f"[cyan][+] Install directory path: {install_path}.",
                        f"[cyan][+] Use `{current_binary_path} help` for more infomation!",
                        f"[cyan][+] Task successfully in {end_time - start_time:.2f}s"
                    ]))

                else:
                    self.console.print("\n".join([
                        f"[cyan][+] Install {current_binary_path} ",
                          f"[cyan][+] Install directory path: {install_path}.",
                        f"[cyan][+] Use `{install_path} help` for more infomation!",
                        f"[cyan][+] Task successfully in {end_time - start_time:.2f}s"
                    ]))
                    
        except KeyboardInterrupt:
            self.console.print("\n[red][!] Install update aborted.")
            sys.exit(1)
        
        except Exception as error:
            self.console.print("\n".join([
                "[red][+] Error when fetch latest verion of Sephera:",
                f"[red][+] Error name: {type(error).__name__}",
                f"[red][+] Error details: [yellow]{error}"
            ]))
            sys.exit(1)