#ifndef WORKER_HPP
#define WORKER_HPP
#include <QObject>
#include <QString>
#include "../../core/include/loc_count.hpp"
#include "qtmetamacros.h"
#include <map>

using namespace std;
using namespace sephera_cpp::core;

namespace sephera_cpp::context {
    class SepheraWorker : public QObject {
        Q_OBJECT

        public:
            void setProjectPath(const QString path) {
                projectPath = path;
            }
        
        signals:
            void finished(const map<string, map<string, double>> locData);
        
        public slots:
            void run() {
                LocCode locCode;
                LocCounter locCounter(locCode, projectPath.toStdString());

                auto result = locCounter.count_loc();
                emit finished(result);
            }

        private:
            QString projectPath;
    };
}

#endif // WORKER_HPP