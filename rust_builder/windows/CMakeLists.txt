# Сборка ядра на Rust для Windows через Cargokit.
cmake_minimum_required(VERSION 3.14)

set(PROJECT_NAME "ogneliss_core")
project(${PROJECT_NAME} LANGUAGES CXX)

include("../cargokit/cmake/cargokit.cmake")
# На Windows плагин подключается через ссылку в
# windows/flutter/ephemeral/.plugin_symlinks/ogneliss_core/windows, поэтому
# путь к каталогу rust отсчитывается от неё (шесть уровней вверх — корень).
apply_cargokit(${PROJECT_NAME} ../../../../../../rust ogneliss_core "")

# Библиотеки, которые Flutter кладёт рядом с .exe.
set(ogneliss_core_bundled_libraries
  "${${PROJECT_NAME}_cargokit_lib}"
  PARENT_SCOPE
)
