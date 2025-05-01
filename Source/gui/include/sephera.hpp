#ifndef SEPHERA_HPP
#define SEPHERA_HPP

#include <QMainWindow>

namespace sephera_cpp::gui {
    class SepheraWindow : public QMainWindow {
        Q_OBJECT

        public:
            explicit SepheraWindow(QWidget *widget = nullptr);
            ~SepheraWindow();
    };
}

#endif // SEPHERA_HPP