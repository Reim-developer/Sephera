import os
import logging
from sephera.CodeLoc import CodeLoc
from PyQt5.QtCore import (
    QRunnable, QThreadPool, pyqtSignal, QObject, pyqtSlot
)
from PyQt5.QtWidgets import (
    QLineEdit, QTableWidget, QTableWidgetItem, QProgressBar, QMessageBox
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

    def show_result(self, table_widget: QTableWidget, codeLoc: CodeLoc):
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
                comment_result = "N/A" if lang_config.comment_style == "no_comment" else str(comment_line)

                table_widget.setItem(row, 0, QTableWidgetItem(language))

                table_widget.setItem(row, 1, QTableWidgetItem(str(loc_line)))
                table_widget.setItem(row, 2, QTableWidgetItem(comment_result))

                table_widget.setItem(row, 3, QTableWidgetItem(str(empty_line)))
                table_widget.setItem(row, 4, QTableWidgetItem(f"{total_sizeof:.2f}"))

                total_loc_count += loc_line
                total_comment += comment_line
                total_empty += empty_line

                total_project_size += total_sizeof

                language_count += 1
                row += 1

        table_widget.setRowCount(row)

    def set_start_scan_event(self, text_line: QLineEdit, 
                             table_widget: QTableWidget, progress: QProgressBar) -> None:
        project_path = text_line.text()

        if not os.path.exists(project_path):
            msg = QMessageBox()
            msg.setWindowTitle("Not found")

            msg.setText("Directory or project path not found.")
            msg.exec()

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
