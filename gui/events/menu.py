from PyQt5.QtCore import Qt
from PyQt5.QtWidgets import QPushButton, QAction, QMessageBox
from gui.etc.subclass import QTableWidgetSubclass
from gui.events.start_scan import StartScanEvent

class MenuEvent:
    def __init__(self, scanEvent: StartScanEvent = None):
        self.scanEvent = scanEvent

    def set_export_acction(self, action: QAction, 
                           button: QPushButton, table: QTableWidgetSubclass) -> None:
        if table.rowCount() == 0:
            dialog = QMessageBox()
            dialog.setWindowTitle("Not found.")

            dialog.setText("Table is none. Start scan to export")
            dialog.exec()

            return

        button.setText(action.text())

        match action.text():
            case "Export to JSON":
                # Add export to JSON soon.
                pass

        

    def set_sort_action(self, action: QAction, 
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
            
