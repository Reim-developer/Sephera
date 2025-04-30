#include "../include/loc_count.hpp"
#include "../include/loc.hpp"
#include <filesystem>

using namespace std::filesystem;

namespace fuzzy {
    LocCounter::LocCounter(LocCode& code_loc, 
                           const std::string& base_path)
        : code_loc_(code_loc), base_path_(base_path) {
        count_loc();
    }

    const std::map<std::string, std::map<std::string, double>>& LocCounter::count_loc() {
       
            loc_count_ = std::map<std::string, std::map<std::string, double>>{};

            for (const auto& language : code_loc_.get_language_data().get_languages()) 
                (*loc_count_)[language.name] = 
                        {{"loc", 0.0}, {"comment", 0.0}, 
                        {"empty", 0.0}, {"size", 0.0}};
            
            (*loc_count_)["Unknown"] = {
                {"loc", 0.0}, {"comment", 0.0}, 
                {"empty", 0.0}, {"size", 0.0}};

            for (const auto& entry : recursive_directory_iterator(base_path_, 
                                                     directory_options::skip_permission_denied)) {
                if (!entry.is_regular_file()) 
                    continue;

                std::string file_path = entry.path().string();
                auto language = code_loc_.get_language_for_file(file_path);

                if (language) {
                    auto [loc_line, comment_line, empty_line] = code_loc_.count_lines_in_file(file_path, *language);
                    double project_file_size = 0.0;
                   
                    project_file_size = file_size(entry) / (1024.0 * 1024.0);
                   
                    (*loc_count_)[language->name]["loc"] += loc_line;
                    (*loc_count_)[language->name]["comment"] += comment_line;

                    (*loc_count_)[language->name]["empty"] += empty_line;
                    (*loc_count_)[language->name]["size"] += project_file_size;

                } else {
                    (*loc_count_)["Unknown"]["size"] += 0.0;
                }
            }

        return *loc_count_;
    }
}