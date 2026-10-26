# Сборка ядра на Rust для Linux через Cargokit.
cmake_minimum_required(VERSION 3.10)
cmake_policy(SET CMP0079 NEW)

set(PROJECT_NAME "ogneliss_core")
project(${PROJECT_NAME} LANGUAGES CXX)

include("../cargokit/cmake/cargokit.cmake")
apply_cargokit(${PROJECT_NAME} ../../rust ogneliss_core "")
target_link_libraries(${BINARY_NAME} PRIVATE "${${PROJECT_NAME}_cargokit_lib}")

# Библиотеки, которые Flutter кладёт в lib/ рядом с приложением.
set(ogneliss_core_bundled_libraries
  "${${PROJECT_NAME}_cargokit_lib}"
  PARENT_SCOPE
)
