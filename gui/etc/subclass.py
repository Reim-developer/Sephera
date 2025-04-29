from PyQt5.QtWidgets import QTableWidgetItem, QTableWidget

class TableWidgetSubclass(QTableWidgetItem):
    def __lt__(self, other):
        return int(self.text()) < int(other.text())
    
class TableWidgetFloat(QTableWidgetItem):
    def __lt__(self, other):
        return float(self.text()) < float(other.text())

class QTableWidgetSubclass(QTableWidget):
    def __init__(self):
        super().__init__()

        self.horizontalHeader().setStyleSheet("""
            QHeaderView::section {
                background-color: #2f3136;
                color: white;
            }
        """)
        self.verticalHeader().setStyleSheet("""
            QHeaderView::section {
                background-color: #2f3136;
                color: white;
            }
        """)

        self.setStyleSheet("""
            QTableCornerButton::section {
               background-color: #2f3136;
            }
        """) 
