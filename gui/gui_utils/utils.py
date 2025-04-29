from typing import Tuple, List
from PyQt5 import QtWidgets
from PyQt5.QtWidgets import (
    QApplication, QWidget, QPushButton, QLineEdit,
    QProgressBar, QMenu, QAction, QScrollBar
)
from PyQt5.QtGui import QPalette, QColor
from gui.etc.subclass import QTableWidgetSubclass

class GuiUtils:
    def __init__(self):
        self.action: QAction = None
        self.menu_button = QPushButton()

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
                   set_flat: bool = True) -> None:
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
                    border: none;
                    
                }}
                QLineEdit:focus {{
                    border: 1px solid grey;
                }}
        """)
        line_edit.setReadOnly(read_only)

        line_edit.setParent(widget)

    def set_table_result(self, widget: QWidget,
                       table_widget: QTableWidgetSubclass, geometry: Tuple[int, int, int, int],
                       bg_color: str = "#2f3136", fore_color: str = "white") -> None:
        scroll_bar = QScrollBar()
        corner = QPushButton()

        corner.setStyleSheet("background-color: #2f3136")
        scroll_bar.setStyleSheet("""
                background-color: #2f3136;
                color: white;
                border: none;       
        """)

        table_widget.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )

        table_widget.setStyleSheet(f"""
                QTableWidget {{
                    background-color: {bg_color};
                    color: {fore_color};
                    border: none;               
                }}
        """)
        table_widget.setVerticalScrollBar(scroll_bar)
        table_widget.setCornerWidget(corner)

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

    def set_menu(self, widget: QWidget, 
                      menu: QMenu, text: List[str],
                      geometry: Tuple[int, int, int, int], button_text: str) -> None:
        
        
        self.menu_button.setText(button_text)

        for item in text:
            action = QAction(item, self.menu_button)
            menu.addAction(action)
        
        self.menu_button.setGeometry(
            geometry[0], geometry[1],
            geometry[2], geometry[3]
        )
        menu.setStyleSheet("""
            QMenu {
                background-color: #2f3136;
                color: white;
            }
        """)
        
        self.menu_button.setStyleSheet("""
            QPushButton {
                background-color: #2f3136;
                color: white;
                border: none;
            }
            QPushButton::hover {
                border: 1px solid grey
            }
        """)
        self.menu_button.setMenu(menu)
        self.menu_button.setParent(widget)

