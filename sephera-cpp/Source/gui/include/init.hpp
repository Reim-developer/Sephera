#ifndef INIT_HPP
#define INIT_HPP
#include <QPushButton>
#include <QLineEdit>
#include <QLabel>
#include "../../context/include/context.hpp"
#include "components.hpp"

using namespace sephera_cpp::context;

namespace sephera_cpp::gui {
    class Init {
        public:
            Init();
            void setupGui(SepheraWindow *sepheraWindow);

            void setupContext();

        private:
            Components *components;
            SepheraContext *context;

        public:
            QLineEdit *displayPathTextbox;
            QLabel *displayPathDescription;
            QPushButton *open_project_btn;
            QPushButton *scan_project_btn;
            QPushButton *option_btn;
    };

} // namespace sephera_cpp::gui

#endif // INIT_HPP