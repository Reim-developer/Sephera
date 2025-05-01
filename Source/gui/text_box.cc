#include "include/text_box.hpp"

using namespace sephera_cpp::gui;

QLineEdit *LineEdit::setLineEdit(QWidget *widget, QLineEdit *lineEdit, QGridLayout *gridLayout) {
    QHBoxLayout *lineEditLayout = new QHBoxLayout();

    lineEditLayout->addStretch();
    lineEditLayout->addWidget(lineEdit);

    lineEdit->setMinimumSize(100, 100);

    gridLayout->addLayout(lineEditLayout, 1, 0);
    return lineEdit;
}