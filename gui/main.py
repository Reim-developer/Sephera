import sys
from __version__ import SEPHERA_VERSION
from gui.gui_utils.uitls import GuiUtils
from PyQt5.QtWidgets import (
    QApplication, QMainWindow,
)
from PyQt5.QtGui import QPalette

class SepheraGui(QMainWindow):
    def __init__(self, sephera_app: QApplication):
        super().__init__()

        self.win_palette = QPalette()
        self.sephera_app = sephera_app

    def setup_windows(self) -> None:
        guiUtils = GuiUtils()

        self.setWindowTitle(f"Sephera GUI | Version: {SEPHERA_VERSION}")
        self.setFixedSize(600, 600)
        
        guiUtils.set_win_color(widget = self, palette = self.win_palette, color = (47, 49, 54))
        guiUtils.move_center(app = self.sephera_app, widget = self)
        self.show()

if __name__ == "__main__":
    sephera_app = QApplication(sys.argv)
    sephera_gui = SepheraGui(sephera_app = sephera_app)

    sephera_gui.setup_windows()
    sys.exit(sephera_app.exec())
