from typing import Tuple
from PyQt5.QtWidgets import QApplication, QWidget
from PyQt5.QtGui import QPalette, QColor

class GuiUtils:
    def move_center(self, app: QApplication, widget: QWidget) -> None:
        screen = app.primaryScreen()
        screen_geometry = screen.availableGeometry()
        
        screen_center = screen_geometry.center()
        frame_geometry = widget.frameGeometry()
        
        frame_geometry.moveCenter(screen_center)
        widget.move(frame_geometry.topLeft())

    def set_win_color(self, widget: QWidget, 
                      palette: QPalette, color: Tuple[int, int, int]) -> None:
        
        palette.setColor(
            QPalette.ColorRole.Window, 
                QColor(color[0], color[1], color[2]))
        
        widget.setPalette(palette)