#include "gui/include/sephera.hpp"
#include "gui/include/init.hpp"

using namespace sephera_cpp::gui;

SepheraWindow::SepheraWindow(QWidget *widget) : QMainWindow(widget) {
    setWindowTitle("Sephera-cpp");
    
    resize(800, 600);
    setMinimumSize(550, 400);

    centralWidget = new QWidget(this);
    layout = new QGridLayout();
    
    setCentralWidget(centralWidget);
    centralWidget->setLayout(layout);

    Init *init = new Init();
    
    init->setupGui(this);
    init->setupContext(this);
}

SepheraWindow::~SepheraWindow() {
  
}