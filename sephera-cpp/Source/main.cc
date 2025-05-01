#include <QApplication>
#include <QWidget>
#include "gui/include/sephera.hpp"

using namespace sephera_cpp::gui;

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);

    SepheraWindow sephera_window;
    sephera_window.show();

    return app.exec();
}
