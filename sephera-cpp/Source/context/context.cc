#include "include/context.hpp"
#include "include/worker.hpp"
#include "qmessagebox.h"
#include "qnamespace.h"
#include <QFileInfo>
#include <QHeaderView>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QObject>
#include <QPoint>
#include <QString>
#include <QTableWidgetItem>
#include <QThread>
#include <map>
#include <string>

using namespace sephera_cpp::context;
using namespace sephera_cpp::core;
using namespace std;

void SepheraContext::setOptionMenuBehavior(QPushButton *button) {
  QMenu *menuOption = new QMenu();
  menuOption->addAction("GitHub");

  QObject::connect(button, &QPushButton::clicked, [=]() {
    menuOption->exec(button->mapToGlobal(QPoint(0, button->height())));
  });
}

void SepheraContext::updateTableView(
    QTableWidget *tableWidget, const map<string, map<string, double>> &data) {

  tableWidget->setRowCount(0);
  tableWidget->setColumnCount(5);
  tableWidget->horizontalHeader()->setSectionResizeMode(QHeaderView::Stretch);

  tableWidget->setHorizontalHeaderLabels(
      {"Language", "Code Lines", "Comment Lines", "Empty Lines", "Size (MB)"});

  for (const auto &[language, metrics] : data) {
    if (metrics.at("loc") == 0.0 && metrics.at("comment") == 0.0 &&
        metrics.at("empty") == 0.0 && metrics.at("size") == 0.0)

      continue;

    auto setItem = [](const QString &text) {
      QTableWidgetItem *tableItem = new QTableWidgetItem(text);
      tableItem->setFlags(tableItem->flags() & ~Qt::ItemIsEditable);

      return tableItem;
    };

    int row = tableWidget->rowCount();
    tableWidget->insertRow(row);

    tableWidget->setItem(row, 0, setItem(QString::fromStdString(language)));
    tableWidget->setItem(row, 1, setItem(QString::number(metrics.at("loc"))));

    tableWidget->setItem(row, 2,
                         setItem(QString::number(metrics.at("comment"))));
    tableWidget->setItem(row, 3, setItem(QString::number(metrics.at("empty"))));

    tableWidget->setItem(row, 4,
                         setItem(QString::number(metrics.at("size"), 'f', 2)));
  }
}

void SepheraContext::setScanButtonBehavior(QWidget *widget, QPushButton *button,
                                           QLineEdit *lineEdit,
                                           QTableWidget *tableWidget) {
  QObject::connect(button, &QPushButton::clicked, [=]() {
    if (lineEdit->text().isEmpty()) {

      QMessageBox::information(widget, "Information", "Project path is empty");
      return;
    }

    if (!QFileInfo(lineEdit->text()).exists()) {

      QMessageBox::information(widget, "Information",
                               "Project path not found.");
      return;
    }

    string projectPath = string(lineEdit->text().toUtf8().constData());

    QThread *thread = new QThread;
    SepheraWorker *sepheraWorker = new SepheraWorker;

    sepheraWorker->setProjectPath(lineEdit->text());
    sepheraWorker->moveToThread(thread);

    QObject::connect(thread, &QThread::started, sepheraWorker, &SepheraWorker::run);

    QObject::connect(sepheraWorker, &SepheraWorker::finished, widget,
                     [=](const map<string, map<string, double>> &locData) {
                       updateTableView(tableWidget, locData);
                       thread->quit();
                     });

    QObject::connect(sepheraWorker, &SepheraWorker::finished, thread,
                     &QThread::quit);
    QObject::connect(thread, &QThread::finished, sepheraWorker,
                     &QObject::deleteLater);

    thread->start();
  });
}