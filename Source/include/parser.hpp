#ifndef PARSER_HPP
#define PARSER_HPP

#include <string>
#include <map>
#include <optional>
#include <yaml-cpp/exceptions.h>
#include <yaml-cpp/node/node.h>
#include <yaml-cpp/node/parse.h>
#include <yaml-cpp/yaml.h>
#include <vector>

namespace fuzzy {    
    struct CommentStyle {
        std::optional<std::string> single_line;
        std::optional<std::string> multi_line_start;
        std::optional<std::string> multi_line_end;
    };
    
    struct LanguageConfig {
        std::string name;
        std::vector<std::string> extensions;
        std::string comment_style;
    };
    
    class LanguageData {
    private:
        std::map<std::string, CommentStyle> comment_styles_;
        std::vector<LanguageConfig> languages_;
    
    public:
        LanguageData(const std::string& config_path);
        const std::vector<LanguageConfig>& get_languages() const;

        const std::map<std::string, CommentStyle>& get_comment_styles() const;
        std::optional<LanguageConfig> get_language_by_name(const std::string& name) const;

        std::optional<LanguageConfig> get_language_by_extension(const std::string& extension) const;
        std::optional<CommentStyle> get_comment_style(const LanguageConfig& language) const;
    };
}
#endif // PARSER_HPP