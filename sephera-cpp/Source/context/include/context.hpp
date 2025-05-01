#ifndef CONTEXT_HPP
#define CONTEXT_HPP
#include <QPushButton>
#include <QTableWidget>
#include <map>
#include <string>

using namespace std;

namespace sephera_cpp::context {
class SepheraContext {
public:
  void setOptionMenuBehavior(QPushButton *button);
  void setScanButtonBehavior(QWidget *window, QPushButton *button,
                             QLineEdit *lineEdit, QTableWidget *tableWidget);

  void updateTableView(QTableWidget *tableWidget,
                       const map<string, map<string, double>> &data);
};

} // namespace sephera_cpp::context

#endif // CONTEXT_HPP