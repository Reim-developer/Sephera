cmake_minimum_required(VERSION 3.10)
project(sephera-cpp)

set(CMAKE_CXX_STANDARD 17)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

set(src
    Source/main.cc
    Source/sephera.cc

    Source/core/parser.cc
    Source/core/loc.cc
    Source/core/loc_count.cc

    Source/gui/components.cc
    Source/gui/init.cc
)

set(headers
    Source/gui/include/sephera.hpp
    Source/gui/include/components.hpp
    Source/gui/include/init.hpp

    Source/core/include/loc_count.hpp
    Source/core/include/loc.hpp
    Source/core/include/parser.hpp
)

find_package(Qt6 REQUIRED COMPONENTS Widgets)
find_package(yaml-cpp REQUIRED)

add_executable(${PROJECT_NAME} ${src} ${headers})
target_link_libraries(
    ${PROJECT_NAME}
    PRIVATE Qt6::Widgets 
    PRIVATE yaml-cpp 
)
target_compile_options(
    ${PROJECT_NAME} 
    PRIVATE "$<$<CXX_COMPILER_ID:Clang>:-fno-direct-access-external-data>")