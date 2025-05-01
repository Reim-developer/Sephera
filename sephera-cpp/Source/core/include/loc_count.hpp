#ifndef LOC_COUNTER_HPP
#define LOC_COUNTER_HPP

#include "loc.hpp"
#include <string>
#include <map>
#include <optional>

namespace sephera_cpp::core {
    class LocCounter {
    private:
        LocCode& code_loc_;
        std::string base_path_;
        std::optional<std::map<std::string, std::map<std::string, double>>> loc_count_;

    public:
        LocCounter(LocCode& code_loc, const std::string& base_path = ".");

        const std::map<std::string, std::map<std::string, double>>& count_loc();
    };
}
#endif // LOC_COUNTER_HPP