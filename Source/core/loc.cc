#include "../include/loc.hpp"
#include <fstream>
#include <iostream>

using namespace fuzzy;
using namespace std;

LocCode::LocCode(const std::string& config_path) : language_data(config_path) {}

const LanguageData& LocCode::get_language_data() const {
    return language_data;
}

std::optional<LanguageConfig> LocCode::get_language_for_file(const std::string& path) const {
    for (const auto& language : language_data.get_languages()) {
        for (const auto& ext : language.extensions) {

            if (path.length() >= ext.length() && 
                path.substr(path.length() - ext.length()) == ext) 
                return language;   
        }
    }
    return std::nullopt;
}

std::tuple<int, int, int> LocCode::count_lines_in_file(const std::string& file_path, const LanguageConfig& language) {
    int loc_line_count = 0;
    int comment_line_count = 0;

    int empty_line_count = 0;
    int comment_nesting_level = 0;

    auto comment_style_opt = language_data.get_comment_style(language);
    const CommentStyle& comment_style = *comment_style_opt;

    ifstream file(file_path, ios::binary);

    file.seekg(0, ios::end);
    if (file.tellg() == 0) {
        file.close();
        return {0, 0, 0};
    }
    file.seekg(0, ios::beg);

    string line;
    while (getline(file, line)) {
        string trimmed_line = line;

        trimmed_line.erase(0, trimmed_line.find_first_not_of(" \t\r\n"));
        trimmed_line.erase(trimmed_line.find_last_not_of(" \t\r\n") + 1);

        if (trimmed_line.empty()) {
            empty_line_count++;
            continue;
        }

        if (comment_nesting_level == 0 && comment_style.single_line && 
            trimmed_line.find(*comment_style.single_line) == 0) {

            comment_line_count++;
            continue;
        }

        if (comment_nesting_level > 0) {
            comment_line_count++;
            string current_line = trimmed_line;

            while (comment_style.multi_line_start && comment_style.multi_line_end &&
                   (current_line.find(*comment_style.multi_line_start) != string::npos ||
                    current_line.find(*comment_style.multi_line_end) != string::npos)) {

                size_t start_idx = current_line.find(*comment_style.multi_line_start);
                size_t end_idx = current_line.find(*comment_style.multi_line_end);

                if (start_idx != string::npos && 
                    (end_idx == string::npos || start_idx < end_idx)) {
                    comment_nesting_level++;
                    current_line = current_line.substr(start_idx + comment_style.multi_line_start->length());

                } else if (end_idx != string::npos) {
                    comment_nesting_level--;
                    current_line = current_line.substr(end_idx + comment_style.multi_line_end->length());
                } else {
                    break;
                }
            }
            continue;
        }

        if (comment_style.multi_line_start && 
            trimmed_line.find(*comment_style.multi_line_start) != string::npos) {
            size_t start_pos = trimmed_line.find(*comment_style.multi_line_start);
            
            string code_before_comment = trimmed_line.substr(0, start_pos);
            code_before_comment.erase(code_before_comment.find_last_not_of(" \t\r\n") + 1);

            if (!code_before_comment.empty()) {
                loc_line_count++;
                string remaining_line = trimmed_line.substr(start_pos + comment_style.multi_line_start->length());

                if (comment_style.multi_line_end && 
                    remaining_line.find(*comment_style.multi_line_end) == string::npos) {
                    comment_nesting_level = 1;
                }

            } else {
                comment_line_count++;
                string current_line = trimmed_line.substr(start_pos + comment_style.multi_line_start->length());

                while (comment_style.multi_line_start && comment_style.multi_line_end &&
                       (current_line.find(*comment_style.multi_line_start) != string::npos ||
                        current_line.find(*comment_style.multi_line_end) != string::npos)) {

                    size_t start_idx = current_line.find(*comment_style.multi_line_start);
                    size_t end_idx = current_line.find(*comment_style.multi_line_end);

                    if (start_idx != string::npos && 
                        (end_idx == string::npos || start_idx < end_idx)) {
                        comment_nesting_level++;
                        current_line = current_line.substr(start_idx + comment_style.multi_line_start->length());

                    } else if (end_idx != string::npos) {
                        comment_nesting_level--;
                        current_line = current_line.substr(end_idx + comment_style.multi_line_end->length());

                    } else {
                        break;
                    }
                }

                if (comment_style.multi_line_end && trimmed_line.find(*comment_style.multi_line_end) == string::npos) 
                    comment_nesting_level = 1;
            }
            continue;
        }

        loc_line_count++;
    }

    file.close();
    return {loc_line_count, comment_line_count, empty_line_count};
}