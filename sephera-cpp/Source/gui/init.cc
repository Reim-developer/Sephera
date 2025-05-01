#include "include/init.hpp"
#include "../context/include/context.hpp"
#include "include/components.hpp"
#include "include/sephera.hpp"

using namespace sephera_cpp::context;
using namespace sephera_cpp::gui;

Init::Init() {
  components = new Components();
  context = new SepheraContext();

  displayPathTextbox = new QLineEdit();
  displayPathDescription = new QLabel();

  open_project_btn = new QPushButton();
  scan_project_btn = new QPushButton();
  option_btn = new QPushButton();
  sort_btn = new QPushButton();

  resultTable = new QTableWidget();
}

void Init::setupGui(SepheraWindow *sepheraWindow) {
  components->setPushButton(sepheraWindow, sort_btn, sepheraWindow->layout, 30,
                            30, 0, 0, "Sort by...");
  components->setPushButton(sepheraWindow, option_btn, sepheraWindow->layout,
                            30, 30, 0, 1, "Options");

  components->setTableWidget(sepheraWindow, resultTable, sepheraWindow->layout,
                             150, 40, 1, 0);
  sepheraWindow->layout->removeWidget(resultTable);
  sepheraWindow->layout->addWidget(resultTable, 1, 0, 1, 3);

  components->setLabel(displayPathDescription, sepheraWindow->layout, 150, 20,
                       2, 0, "Input project path or section project");
  displayPathDescription->setWordWrap(true);
  sepheraWindow->layout->removeWidget(displayPathDescription);
  sepheraWindow->layout->addWidget(displayPathDescription, 2, 0, 1, 2);

  components->setLineEdit(displayPathTextbox, sepheraWindow->layout, 150, 30, 3,
                          0);
  sepheraWindow->layout->removeWidget(displayPathTextbox);
  sepheraWindow->layout->addWidget(displayPathTextbox, 3, 0, 1, 2);

  components->setPushButton(sepheraWindow, open_project_btn,
                            sepheraWindow->layout, 30, 30, 3, 2,
                            "Open Project");
  components->setPushButton(sepheraWindow, scan_project_btn,
                            sepheraWindow->layout, 30, 30, 3, 3,
                            "Scan Project");
}

void Init::setupContext(SepheraWindow *sepheraWindow) {
  context->setOptionMenuBehavior(this->option_btn);

  context->setScanButtonBehavior(sepheraWindow, scan_project_btn,
                                 displayPathTextbox, resultTable);
}
