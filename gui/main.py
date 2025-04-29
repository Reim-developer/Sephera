import sys
import logging
from __version__ import SEPHERA_VERSION
from gui.gui_utils.utils import GuiUtils
from gui.setup_app import Setup
from PyQt5.QtWidgets import (
    QApplication, QMainWindow
)
from PyQt5.QtGui import QPalette

class SepheraGui(QMainWindow):
    def __init__(self, sephera_app: QApplication):
        super().__init__()
        logging.basicConfig(level = logging.DEBUG, format = "%(levelname)s - %(message)s")

        self.setup = Setup(widget = self)
        self.guiUtils = GuiUtils()

        self.win_palette = QPalette()
        self.sephera_app = sephera_app

    def setup_windows(self) -> None:
        self.setWindowTitle(f"Sephera GUI | Version: {SEPHERA_VERSION}")
        self.setFixedSize(800, 600)
        
        self.setup.setup_application(app = self.sephera_app)
        self.show()

if __name__ == "__main__":
    try:
        sephera_app = QApplication(sys.argv)
        sephera_gui = SepheraGui(sephera_app = sephera_app)

        sephera_gui.setup_windows()
        logging.info(f"Started Sephera - GUI version: {SEPHERA_VERSION}")

        sephera_app.exec()
        sys.exit(0)
    
    except KeyboardInterrupt:
        logging.info("\nKeyboard interrupt.")
        sys.exit(1)