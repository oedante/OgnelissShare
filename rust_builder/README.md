# ogneliss_core — сборка ядра

Плагин Flutter без кода на Dart: подключает к приложению библиотеку из
`../rust` (крейт `ogneliss_core`). Сборку делает
[Cargokit](https://github.com/irondash/cargokit) (MIT, копия — в `cargokit/`,
версия из flutter_rust_bridge, совместимая с Gradle 9):

| Платформа | Что собирается | Как подключается |
|---|---|---|
| Android | `libogneliss_core.so` для каждой ABI | `jniLibs` через Gradle (`android/build.gradle`) |
| iOS, macOS | `libogneliss_core.a` | CocoaPods, `-force_load` в фреймворк `ogneliss_core` |
| Windows | `ogneliss_core.dll` | CMake, рядом с `.exe` |
| Linux | `libogneliss_core.so` | CMake, в `lib/` сборки |

Нужен установленный Rust (`rustup`): цели (`aarch64-linux-android` и др.)
Cargokit добавляет сам. Для Android — NDK той версии, что указана в
`android/app/build.gradle.kts` (`flutter.ndkVersion`).
