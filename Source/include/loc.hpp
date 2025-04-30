#ifndef LOC_HPP
#define LOC_HPP
#include "parser.hpp"
#include <string>
#include <tuple>

using namespace fuzzy;

namespace fuzzy {
    class LocCode {
    private:
        LanguageData language_data;

    public:
        LocCode(const std::string& config_path = "cfg.yml");

        const LanguageData& get_language_data() const;

        std::tuple<int, int, int> count_lines_in_file(const std::string& file_path, 
                                                      const LanguageConfig& language);

        std::optional<LanguageConfig> get_language_for_file(const std::string& path) const;
    };
}
#endif