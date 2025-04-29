from PyQt5.QtWidgets import (
    QPushButton, QWidget, QApplication, QLineEdit,
    QProgressBar, QMenu
)
from PyQt5.QtGui import QPalette
from gui.gui_utils.utils import GuiUtils
from gui.events.open_project import OpenProjectEvent
from gui.events.start_scan import StartScanEvent
from gui.events.menu import MenuEvent
from gui.etc.subclass import QTableWidgetSubclass

class Setup:
    def __init__(self, widget: QWidget):
        self.widget = widget
        self.gui_utils = GuiUtils()

        self.open_project_btn = QPushButton()
        self.start_scan_btn = QPushButton()

        self.show_project_line = QLineEdit()
        self.show_result_table = QTableWidgetSubclass()

        self.win_palette = QPalette()
        self.progress_bar = QProgressBar()

        self.open_prj_event = OpenProjectEvent()
        self.scan_prj_event = StartScanEvent()

        self.sort_by_menu = QMenu()
        self.menu_event = MenuEvent(scanEvent = self.scan_prj_event)

    def setup_event(self) -> None:
        self.open_project_btn.clicked.connect(
            lambda:
                self.open_prj_event.set_open_project_event(line_edit = self.show_project_line)
        )
        self.start_scan_btn.clicked.connect(
            lambda:
                self.scan_prj_event.set_start_scan_event(
                    text_line = self.show_project_line, 
                    table_widget = self.show_result_table, progress = self.progress_bar)
        )

        self.sort_by_menu.triggered.connect(
            lambda action:
                self.menu_event.set_menu_action(
                    action,
                    self.gui_utils.menu_button, self.show_result_table
                )
        )

    def setup_application(self, app: QApplication) -> None:
        self.gui_utils.set_button(
            widget = self.widget, button = self.open_project_btn,
            text = "Open Project", geometry = (0, 500, 250, 30)
        )
        self.gui_utils.set_button(
            widget = self.widget, button = self.start_scan_btn,
            text = "Start Scan", geometry = (300, 500, 250, 30)
        )
        
        self.gui_utils.set_line_edit(
            widget = self.widget, line_edit = self.show_project_line,
            text = "Nothing to show, open a project to start scan.",
            geometry = (0, 450, 600, 40)
        )

        self.gui_utils.set_table_result(
            widget = self.widget, table_widget = self.show_result_table,
            geometry = (0, 100, 550, 300)
        )
       
        self.gui_utils.set_win_color(
            widget = self.widget, 
            palette = self.win_palette, color = (47, 49, 54))
        
        self.gui_utils.move_center(
            app = app, widget = self.widget
        )

        self.gui_utils.set_progress_bar(
            widget = self.widget, progress = self.progress_bar,
            geometry = (300, 420, 250, 30)
        )

        self.gui_utils.set_menu(
            widget = self.widget, menu = self.sort_by_menu,
            text = [
                "Most lines of code", "Least lines of code",
                "Most lines of comment", "Least lines of comment",
                "Most lines of empty", "Least lines of empty",
                "Most size", "Least size"
            ], geometry = (0, 50, 150, 30),
            button_text = "Sort by..."
        )

        self.setup_event()