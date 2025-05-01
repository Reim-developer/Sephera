#include "include/parser.hpp"

using namespace sephera_cpp::core;

LanguageData::LanguageData(const std::string& config_path) {
    YAML::Node config = YAML::LoadFile(config_path);

    for (const auto& style : config["comment_styles"]) {
            CommentStyle comment_style;
            const auto& style_node = style.second;

            if (style_node["single_line"]) 
                comment_style.single_line = style_node["single_line"].as<std::string>();
                
                
            if (style_node["multi_line_start"]) 
                comment_style.multi_line_start = style_node["multi_line_start"].as<std::string>();
            
        
            if (style_node["multi_line_end"]) {
                comment_style.multi_line_end = style_node["multi_line_end"].as<std::string>();
            }

                comment_styles_[style.first.as<std::string>()] = comment_style;
            }

       
        for (const auto& lang : config["languages"]) {
            LanguageConfig language_config;

            language_config.name = lang["name"].as<std::string>();
            language_config.comment_style = lang["comment_styles"].as<std::string>();
                
            for (const auto& ext : lang["extension"]) {
                if (ext.IsScalar()) 
                    language_config.extensions.push_back(ext.as<std::string>());
                    
            }
                languages_.push_back(language_config);
        }
}

const std::vector<LanguageConfig>& LanguageData::get_languages() const {
    return languages_;
}

const std::map<std::string, CommentStyle>& LanguageData::get_comment_styles() const {
    return comment_styles_;
}

std::optional<LanguageConfig> LanguageData::get_language_by_name(const std::string& name) const {
    for (const auto& lang : languages_) {

        if (lang.name == name) return lang;
    }
    return std::nullopt;
}

std::optional<LanguageConfig> LanguageData::get_language_by_extension(const std::string& extension) const {
    for (const auto& lang : languages_) {

        for (const auto& ext : lang.extensions) 
            if (ext == extension) return lang;     
    }
    return std::nullopt;
}

std::optional<CommentStyle> LanguageData::get_comment_style(const LanguageConfig& language) const {
    auto it = comment_styles_.find(language.comment_style);

    if (it != comment_styles_.end()) return it->second;
    return std::nullopt;
}