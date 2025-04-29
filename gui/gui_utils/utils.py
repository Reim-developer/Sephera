from typing import Tuple
from PyQt5 import QtWidgets
from PyQt5.QtWidgets import (
    QApplication, QWidget, QPushButton, QLineEdit,
    QTableWidget, QProgressBar
)
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

    def set_button(self, widget: QWidget, 
                   button: QPushButton, text: str,
                   geometry: Tuple[int, int, int, int],
                   bg_color: str = "#2f3136", fore_color: str = "white", 
                   set_flat: bool = False) -> None:
        button.setText(text)
        button.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )
        
        button.setFlat(set_flat)
        button.setStyleSheet(f"""
                background-color: {bg_color};
                color: {fore_color};
        """)
        button.setParent(widget)

    def set_line_edit(self, widget: QWidget,
                       line_edit: QLineEdit, text: str,
                       geometry: Tuple[int, int, int, int],
                       bg_color: str = "#2f3136", fore_color: str = "white",
                       read_only: bool = True) -> None:
        line_edit.setText(text)
        line_edit.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )

        line_edit.setStyleSheet(f"""
                QLineEdit {{
                    background-color: {bg_color};
                    color: {fore_color};
                    border: 1px solid #2f3136;
                    
                }}
                QLineEdit:focus {{
                    border: 1px solid grey;
                }}
        """)
        line_edit.setReadOnly(read_only)
        line_edit.setFocus

        line_edit.setParent(widget)

    def set_table_result(self, widget: QWidget,
                       table_widget: QTableWidget, geometry: Tuple[int, int, int, int],
                       bg_color: str = "#2f3136", fore_color: str = "white") -> None:
        table_widget.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )

        table_widget.setStyleSheet(f"""
                QTableWidget {{
                    background-color: {bg_color};
                    color: {fore_color};               
                }}

                QTableWidget:focus {{
                    border: 1px solid grey;
                }}
        """)

        table_widget.setEditTriggers(QtWidgets.QAbstractItemView.EditTrigger.NoEditTriggers)
        table_widget.setParent(widget)

    
    def set_progress_bar(self, widget: QWidget,
                         progress: QProgressBar, geometry: Tuple[int, int, int, int]) -> None:
        
        progress.setStyleSheet("""
            QProgressBar {
                background-color: #2f3136;
                border: none;
            }
              
            QProgressBar::chunk {
                background-color: #4caf50;
                width: 20px;
            }
        """)

        progress.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )
        progress.setParent(widget)
