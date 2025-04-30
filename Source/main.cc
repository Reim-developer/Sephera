#include <QApplication>
#include <QWidget>

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);

    QWidget window;
    window.setWindowTitle("Sephera-cpp");
    window.resize(600, 600);
    window.show();

    return app.exec();
}
