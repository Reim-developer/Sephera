#include "include/init.hpp"
#include "include/components.hpp"
#include "include/sephera.hpp"
#include "../context/include/context.hpp"

using namespace sephera_cpp::context;
using namespace sephera_cpp::gui;

Init::Init() {
    Components components;
    context = new SepheraContext();

    displayPathTextbox = new QLineEdit();
    displayPathDescription = new QLabel();
  
    open_project_btn = new QPushButton();
    scan_project_btn = new QPushButton();
    option_btn = new QPushButton();
}

void Init::setupGui(SepheraWindow *sepheraWindow) {
  
  components.setSpacerItem(sepheraWindow, 20, 20, 1, 1);

  components.setLineEdit(sepheraWindow, displayPathTextbox,
                          sepheraWindow->layout, 150, 30, 4, 1);

  components.setLabel(sepheraWindow, displayPathDescription,
                       sepheraWindow->layout, 150, 30, 3, 1, 
                       QString("Input project path or section project"));

  components.setPushButton(sepheraWindow, open_project_btn,
                           sepheraWindow->layout, 
                           30, 30, 4, 2, "Open Project");
   
  components.setPushButton(sepheraWindow, scan_project_btn,
                            sepheraWindow->layout, 
                            30, 30, 4, 3, "Scan Project");

  components.setPushButton(sepheraWindow, option_btn,
                           sepheraWindow->layout, 
                           30, 30, 0, 3, "Options");
}

void Init::setupContext() {
  context->setOptionMenuBehavior(this->option_btn);
}
