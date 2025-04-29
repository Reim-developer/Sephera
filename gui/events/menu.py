from PyQt5.QtCore import Qt
from PyQt5.QtWidgets import QPushButton, QAction
from gui.etc.subclass import QTableWidgetSubclass
from gui.events.start_scan import StartScanEvent

class MenuEvent:
    def __init__(self, scanEvent: StartScanEvent = None):
        self.scanEvent = scanEvent
        
    def set_menu_action(self, action: QAction, 
                        button: QPushButton, table: QTableWidgetSubclass) -> None:
        button.setText(action.text())
        self.scanEvent.set_sort_mode(action.text())

        match action.text():
            # Most, least lines of code
            case "Most lines of code":
                table.sortItems(1, Qt.SortOrder.DescendingOrder)

            case "Least lines of code":
                table.sortItems(1, Qt.SortOrder.AscendingOrder)

            # Most, least lines of comment
            case "Most lines of comment":
                table.sortItems(2, Qt.SortOrder.DescendingOrder)

            case "Least lines of comment":
                table.sortItems(2, Qt.SortOrder.AscendingOrder)

            # Most, least empty lines
            case "Most lines of empty":
                table.sortItems(3, Qt.SortOrder.DescendingOrder)

            case "Least lines of empty":
                table.sortItems(3, Qt.SortOrder.AscendingOrder)
            
            # Most, least sizeof
            case "Most size":
                table.sortItems(4, Qt.SortOrder.DescendingOrder)

            case "Least size":
                table.sortItems(4, Qt.SortOrder.AscendingOrder)
            
