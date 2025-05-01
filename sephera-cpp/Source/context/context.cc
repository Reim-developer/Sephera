#include "include/context.hpp"
#include <QPoint>
#include <QObject>
#include <QMenu>
#include <qdebug.h>

using namespace sephera_cpp::context;

void SepheraContext::setOptionMenuBehavior(QPushButton *button) {
    QMenu *menuOption = new QMenu();
    menuOption->addAction("GitHub");

    QObject::connect(button, &QPushButton::clicked, [=]() {

        menuOption->exec(button->mapToGlobal(QPoint(0, button->height())));
    });
}