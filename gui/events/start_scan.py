import os
import logging
from sephera.CodeLoc import CodeLoc
from gui.etc.subclass import (
    TableWidgetSubclass, TableWidgetFloat,
    QTableWidgetSubclass
)
from PyQt5.QtCore import (
    QRunnable, QThreadPool, pyqtSignal, QObject, pyqtSlot, Qt
)
from PyQt5.QtWidgets import (
    QLineEdit, QTableWidgetItem, QProgressBar, QMessageBox,
)

class ScanWorkerSignals(QObject):
    finished = pyqtSignal(object) 

class ScanWorker(QRunnable):
    def __init__(self, path: str):
        super().__init__()
        self.signals = ScanWorkerSignals()
        self.path = path

    @pyqtSlot()
    def run(self):
        codeLoc = CodeLoc(base_path=self.path)
        self.signals.finished.emit(codeLoc)


class StartScanEvent:
    def __init__(self):
        self.threadpool = QThreadPool()
        self._sort_mode: str | None = None

    def set_sort_mode(self, mode: str) -> None:
        self._sort_mode = mode

    def _auto_sort(self, table: QTableWidgetSubclass) -> None:
         match self._sort_mode:
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

    def show_result(self, table_widget: QTableWidgetSubclass, codeLoc: CodeLoc):
        table_widget.setColumnCount(5)
        table_widget.setHorizontalHeaderLabels([
            "Language", "Code lines", "Comment lines", "Empty lines", "Size (MB)"
        ])

        table_widget.setRowCount(len(codeLoc._loc_count))
        total_loc_count = total_comment = total_empty = total_project_size = language_count = row = 0

        for language, count in codeLoc._loc_count.items():
            loc_line = count["loc"]
            comment_line = count["comment"]

            empty_line = count["empty"]
            total_sizeof = count["size"]

            if loc_line > 0 or comment_line > 0 or empty_line > 0 or total_sizeof > 0:
                lang_config = codeLoc.language_data.get_language_by_name(name=language)
                comment_result = 0 if lang_config.comment_style == "no_comment" else str(comment_line)

                table_widget.setItem(row, 0, QTableWidgetItem(language))

                table_widget.setItem(row, 1, TableWidgetSubclass(str(loc_line)))
                table_widget.setItem(row, 2, TableWidgetSubclass(str(comment_result)))

                table_widget.setItem(row, 3, TableWidgetSubclass(str(empty_line)))
                table_widget.setItem(row, 4, TableWidgetFloat(f"{total_sizeof:.2f}"))

                total_loc_count += loc_line
                total_comment += comment_line
                total_empty += empty_line

                total_project_size += total_sizeof

                language_count += 1
                row += 1

        table_widget.setRowCount(row)

        self._auto_sort(table = table_widget)

    def set_start_scan_event(self, text_line: QLineEdit, 
                             table_widget: QTableWidgetSubclass, progress: QProgressBar) -> None:
        project_path = text_line.text()

        if not os.path.exists(project_path):
            dialog = QMessageBox()
            dialog.setWindowTitle("Not found")

            dialog.setText("Directory or project path not found.")
            dialog.exec()

            logging.warning("Directory or project path not found.")
            return
        
        progress.setRange(0, 0)
        worker = ScanWorker(project_path)

        worker.signals.finished.connect(
            lambda result: (
                self.show_result(table_widget, result),
                progress.setRange(0, 1)
        ))
        self.threadpool.start(worker)
