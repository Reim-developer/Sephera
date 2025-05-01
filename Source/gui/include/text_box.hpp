#ifndef TEXT_BOX_HPP
#define TEXT_BOX_HPP

#include <QLineEdit>
#include <QGridLayout>
#include <QWidget>

namespace sephera_cpp::gui {
    class LineEdit {
        public:
            QLineEdit *setLineEdit(QWidget *widget, QLineEdit *lineEdit,
                                   QGridLayout *gridLayout);
    };
}

#endif // TEXT_BOX_HPP