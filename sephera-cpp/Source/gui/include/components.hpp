#ifndef TEXT_BOX_HPP
#define TEXT_BOX_HPP
#include <QLabel>
#include "sephera.hpp"
#include <QLineEdit>
#include <QPushButton>
#include <QGridLayout>
#include <QWidget>
#include <QString>

namespace sephera_cpp::gui {
    class Components {
        public:
            QLineEdit *setLineEdit(QLineEdit *lineEdit,
                                   QGridLayout *gridLayout, int width, int height,
                                   int row, int column);

            QLabel *setLabel(QLabel *label,
                             QGridLayout *gridLayout, int width, int height,
                             int row, int column, const QString& text);
            
            QSpacerItem *setSpacerItem(SepheraWindow *sepheraWindow,
                                       int width, int height,
                                       int row, int column);

            QPushButton *setPushButton(SepheraWindow *sepheraWindow, QPushButton *button,
                                       QGridLayout *layout, int width, int height,
                                       int row, int column, const QString &text);
    };
} // namespace sephera_cpp::gui

#endif // TEXT_BOX_HPP