cmake_minimum_required(VERSION 3.10)
project(sephera-cpp)

set(CMAKE_CXX_STANDARD 17)

set(src
    Source/main.cc
    Source/core/parser.cc
    Source/core/loc.cc
    Source/core/loc_count.cc
)

find_package(Qt6 REQUIRED COMPONENTS Widgets)
find_package(yaml-cpp REQUIRED)

add_executable(${PROJECT_NAME} ${src})
target_link_libraries(
    ${PROJECT_NAME}
    PRIVATE Qt6::Widgets 
    PRIVATE yaml-cpp 
)

set(CMAKE_AUTOMOC ON)
target_compile_options(
    ${PROJECT_NAME} 
    PRIVATE "$<$<CXX_COMPILER_ID:Clang>:-fno-direct-access-external-data>")