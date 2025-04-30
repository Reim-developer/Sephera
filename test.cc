// #include "include/loc_count.hpp"
// #include <filesystem>
// #include <iostream>
// #include <iomanip>

// using namespace fuzzy;
// namespace fs = std::filesystem;

// void test_loc_counter() {
//     try {
//         // Khởi tạo LocCode với cfg.yml
//         LocCode code_loc("config.yml");

//         // Khởi tạo LocCounter với thư mục mục tiêu
//         LocCounter loc_counter(code_loc, "/home/reim/Codes");

//         // Gọi count_loc
//         const auto& loc_count = loc_counter.count_loc();

//         // In tiêu đề
//         std::cout << "LOC count of directory: " << fs::absolute("/home/reim/Codes").string() << "\n";
//         std::cout << std::left << std::setw(20) << "Language"
//                   << std::right << std::setw(15) << "Code lines"
//                   << std::setw(20) << "Comments lines"
//                   << std::setw(15) << "Empty lines"
//                   << std::setw(15) << "Size (MB)" << "\n";
//         std::cout << std::string(85, '-') << "\n";

//         // In kết quả cho từng ngôn ngữ
//         double total_loc = 0.0;
//         double total_comment = 0.0;
//         double total_empty = 0.0;
//         double total_size = 0.0;
//         int language_count = 0;

//         for (const auto& [language, counts] : loc_count) {
//             double loc = counts.at("loc");
//             double comment = counts.at("comment");
//             double empty = counts.at("empty");
//             double size = counts.at("size");

//             if (loc > 0 || comment > 0 || empty > 0 || size > 0) {
//                 language_count++;
//                 auto lang_config = code_loc.get_language_data().get_language_by_name(language);
//                 std::string comment_result = (lang_config && lang_config->comment_style == "no_comment") 
//                                            ? "N/A" 
//                                            : std::to_string(static_cast<int>(comment));

//                 std::cout << std::left << std::setw(20) << language
//                           << std::right << std::setw(15) << static_cast<int>(loc)
//                           << std::setw(20) << comment_result
//                           << std::setw(15) << static_cast<int>(empty)
//                           << std::setw(15) << std::fixed << std::setprecision(2) << size << "\n";
//                 total_loc += loc;
//                 total_comment += comment;
//                 total_empty += empty;
//                 total_size += size;
//             }
//         }

//         // In tóm tắt
//         std::cout << std::string(85, '-') << "\n";
//         std::cout << "[+] Code: " << static_cast<int>(total_loc) << " lines\n";
//         std::cout << "[+] Comments: " << static_cast<int>(total_comment) << " lines\n";
//         std::cout << "[+] Empty: " << static_cast<int>(total_empty) << " lines\n";
//         std::cout << "[+] Language(s) used: " << language_count << " language(s)\n";
//         std::cout << "[+] Total Project Size: " << std::fixed << std::setprecision(2) << total_size << " MB\n";
//     } catch (const std::exception& e) {
//         std::cerr << "Error: " << e.what() << "\n";
//     }
// }

// int main() {
//     test_loc_counter();
//     return 0;
// }