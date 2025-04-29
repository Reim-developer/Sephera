import logging
from PyQt5.QtWidgets import QFileDialog, QLineEdit

class OpenProjectEvent:
    def set_open_project_event(self, line_edit: QLineEdit) -> None:
        project_path = QFileDialog.getExistingDirectory(caption = "Select project")

        if project_path: 
            line_edit.setText(project_path)
            logging.info(f"Project selected in: {project_path}")

        else:
            logging.info("Project selection canceled.")
