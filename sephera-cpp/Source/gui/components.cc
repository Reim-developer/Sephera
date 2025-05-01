#include "include/components.hpp"
#include "include/sephera.hpp"
#include <qdebug.h>

using namespace sephera_cpp::gui;

QLineEdit *Components::setLineEdit(QLineEdit *lineEdit,
                                   QGridLayout *gridLayout, int width, int height,
                                   int row, int column) {
  
  lineEdit->setMinimumSize(width, height);
  gridLayout->addWidget(lineEdit, row, column);

  return lineEdit;
}

QLabel *Components::setLabel(QLabel *label,
                             QGridLayout *gridLayout, int width, int height,
                             int row, int column, const QString& text) {
  
  label->setText(text);
  label->setMinimumSize(width, height);
  gridLayout->addWidget(label, row, column);

  return label;
}

QSpacerItem *Components::setSpacerItem(SepheraWindow *sepheraWindow,
                                       int width, int height,
                                       int row, int column) {
  QSpacerItem *spacerItem = new QSpacerItem(width, height, QSizePolicy::Minimum, QSizePolicy::Expanding);
  
  

  sepheraWindow->layout->addItem(spacerItem, row, column);

  return spacerItem;
}

QPushButton *Components::setPushButton(SepheraWindow *sepheraWindow, QPushButton *button,
                                       QGridLayout *layout, int width, int height,
                                       int row, int column, const QString &text) {
  
  button->setText(text);
  button->setParent(sepheraWindow);
  
  button->setMinimumSize(width, height);
  layout->addWidget(button, row, column);
  
  return button;
}

QTableWidget *Components::setTableWidget(SepheraWindow *sepheraWindow, QTableWidget *tableWidget,
                                         QGridLayout *layout, const int width, const int height,
                                         const int row, const int column) {
    
    tableWidget->setRowCount(0);
    tableWidget->setMinimumSize(width, height);

    tableWidget->setParent(sepheraWindow);
    layout->addWidget(tableWidget, row, column);
    
    return tableWidget;
  }
  