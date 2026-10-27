# Сборка для Android и Windows

> macOS, iOS, Linux и выпуск в Google Play, App Store и Microsoft Store —
> в [RELEASE.md](RELEASE.md).

Инструкция для компьютера с Windows 10/11 (64-бит): на нём собираются и
приложение для Windows, и APK для Android. На Linux и macOS шаги те же,
меняются только способы установки программ.

Приложение состоит из двух частей: ядро на Rust (`rust/`) и интерфейс на
Flutter (`lib/`). Ядро собирается само во время сборки Flutter, отдельно
его запускать не нужно.

> **Без сборки.** Готовые файлы для проверки собирает GitHub при каждом
> изменении: [Actions](https://github.com/oedante/OgnelissShare/actions) →
> последний зелёный запуск **CI** → внизу страницы **Artifacts** →
> `ogneliss-share-android` (APK) и `ogneliss-share-windows` (папка с
> программой). Нужен вход в GitHub; файлы хранятся 14 дней.

## 1. Что установить (один раз)

| Программа | Зачем | Где взять |
|---|---|---|
| Git | скачать код | https://git-scm.com/download/win |
| Python 3.10+ | скрипт подготовки `tool/bootstrap.py` | https://www.python.org/downloads/windows/ — отметьте «Add python.exe to PATH» |
| Rust (rustup) | ядро | https://rustup.rs — `rustup-init.exe`, ответы по умолчанию |
| Visual Studio 2022 Community | сборка для Windows и компилятор для Rust | https://visualstudio.microsoft.com/ru/downloads/ — нагрузка **«Разработка классических приложений на C++»** |
| Flutter (stable) | интерфейс и сборка | см. ниже |
| Android Studio | только для Android: SDK, NDK, Java | https://developer.android.com/studio |

То же самое через `winget` (PowerShell, затем откройте терминал заново):

```powershell
winget install -e --id Git.Git
winget install -e --id Python.Python.3.12
winget install -e --id Rustlang.Rustup
winget install -e --id Microsoft.VisualStudio.2022.Community --override "--add Microsoft.VisualStudio.Workload.NativeDesktop --includeRecommended --passive"
winget install -e --id Google.AndroidStudio
```

**Flutter.** Короткий путь без пробелов и русских букв, например `C:\src`:

```powershell
git clone https://github.com/flutter/flutter.git -b stable C:\src\flutter
$p = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$p;C:\src\flutter\bin", "User")
```

Откройте терминал заново и проверьте: `flutter --version` (проверено на
Flutter 3.47, Rust 1.97).

**Режим разработчика Windows** — без него Flutter не соберёт программу с
плагинами (ошибка «Building with plugins requires symlink support»):

```powershell
start ms-settings:developers
```

и включите «Режим разработчика».

**Для Android** — запустите Android Studio один раз (мастер установит SDK),
затем *More Actions → SDK Manager → SDK Tools* и отметьте **Android SDK
Command-line Tools (latest)**. NDK нужной версии сборка поставит сама.
Примите лицензии:

```powershell
flutter doctor --android-licenses
```

Итог: `flutter doctor` без ошибок в строках *Windows Version*,
*Visual Studio* и (для Android) *Android toolchain*. Строки про Chrome и
Xcode не важны.

## 2. Скачать код и подготовить проект

```powershell
cd C:\src
git clone https://github.com/oedante/OgnelissShare.git
cd OgnelissShare
python tool\bootstrap.py
```

`bootstrap.py` создаёт каталоги платформ (`android\`, `windows\` и
остальные) через `flutter create`, накладывает на них свои файлы из
`tool\platform` (значки, разрешения, «Поделиться» на Android),
скачивает пакеты Dart и создаёт классы переводов. Занимает около минуты;
повторный запуск безопасен.

## 3. Windows

```powershell
flutter build windows --release
```

Первая сборка — 10–20 минут (ядро на Rust собирается с полной
оптимизацией), следующие — быстрее. Результат — папка
`build\windows\x64\runner\Release\`:

- `ogneliss_share.exe` — программа;
- рядом её библиотеки (`flutter_windows.dll`, `ogneliss_core.dll` и др.)
  и папка `data\` — переносите папку целиком.

Запуск: `build\windows\x64\runner\Release\ogneliss_share.exe`. Для
отладки с журналом в терминале — `flutter run -d windows`.

При первом запуске Windows спросит доступ к сети — **разрешите для
частных сетей**: без этого устройства не найдут друг друга напрямую в
локальной сети (через интернет связь всё равно будет, но медленнее).

На другом компьютере может не хватить библиотек Visual C++ — тогда
установите [Visual C++ Redistributable x64](https://aka.ms/vs/17/release/vc_redist.x64.exe).

## 4. Android

Телефон с Android 7.0 и новее.

**Быстрый способ — по кабелю.** На телефоне включите *Параметры
разработчика → Отладка по USB* (параметры разработчика открываются
семью нажатиями на «Номер сборки» в «О телефоне»), подключите кабель,
разрешите отладку на экране телефона и выполните:

```powershell
flutter devices          # телефон должен быть в списке
flutter run --release    # соберёт, установит и запустит
```

**APK-файл.** Для одного современного телефона быстрее собрать только
под 64-битный ARM:

```powershell
flutter build apk --release --target-platform android-arm64
```

Файл: `build\app\outputs\flutter-apk\app-release.apk`. Перекиньте его на
телефон и откройте (разрешите установку из этого источника; Play Защита
может предупредить о неизвестном приложении — «Установить всё равно»).
Или по кабелю:

```powershell
& "$env:LOCALAPPDATA\Android\Sdk\platform-tools\adb.exe" install -r build\app\outputs\flutter-apk\app-release.apk
```

Все варианты сразу (как в CI) — `flutter build apk --release
--split-per-abi`: `app-arm64-v8a-release.apk` для большинства телефонов,
`app-armeabi-v7a-release.apk` для старых 32-битных, `app-x86_64-release.apk`
для эмулятора.

**Подпись.** Без файла `android\key.properties` APK подписан отладочным
ключом — для проверки этого достаточно. Перед установкой сборки с другой
подписью (например, из магазина) отладочную придётся удалить. Свой ключ:
файл `android\key.properties` со строками `storeFile=…`,
`storePassword=…`, `keyAlias=…`, `keyPassword=…` (в git не попадает).

## 5. Первая проверка

1. Запустите приложение на компьютере и на телефоне. Удобнее всего, если
   оба в одной сети Wi-Fi (гостевые сети часто запрещают устройствам
   видеть друг друга — тогда связь пойдёт через интернет).
2. На компьютере — **«Показать код»**, на телефоне — **«Сканировать код»**
   и наведите камеру на экран компьютера.
3. На компьютере появится «Подключить …?» с четырьмя цифрами — сверьте их
   с цифрами на телефоне и нажмите **«Подключить»**.
4. На телефоне нажмите на компьютер в списке → **«Файлы»** или **«Фото и
   видео»**. На компьютере файлы придут сами в папку
   *Загрузки\Ogneliss Share*.
5. В обратную сторону: на компьютере нажмите на телефон → «Файлы»,
   «Папку» или «Текст или ссылку». На телефоне файлы — в
   *Загрузки → Ogneliss Share*.
6. «Поделиться» из любого приложения Android → Ogneliss Share → выбор
   устройства.

Что ещё стоит проверить: обрыв Wi-Fi посреди большой передачи (после
восстановления она продолжится с места остановки), телефон на мобильном
интернете, «Разовый обмен» (получатель подтверждает каждую передачу),
«Забыть устройство».

**Если что-то не так** — *Настройки → Журнал → значок «Копировать»* и
приложите текст к [Issue](https://github.com/oedante/OgnelissShare/issues)
вместе с описанием: что делали, на каких устройствах, в одной ли сети.

## 6. Если сборка не идёт

- **`Не найден flutter в PATH`** — откройте терминал заново после
  установки Flutter; проверьте `flutter --version`.
- **`Building with plugins requires symlink support`** — включите режим
  разработчика (раздел 1).
- **Ошибки про NDK или `sdkmanager`** — в SDK Manager отметьте *Android
  SDK Command-line Tools (latest)*, выполните
  `flutter doctor --android-licenses`.
- **Ошибки длинных путей** (`path too long`, `Filename longer than 260
  characters`) — держите проект в коротком пути (`C:\src\OgnelissShare`),
  выполните `git config --global core.longpaths true` и включите длинные
  пути Windows (PowerShell от администратора):
  `New-ItemProperty -Path HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem -Name LongPathsEnabled -Value 1 -PropertyType DWORD -Force`.
- **Имя пользователя Windows русскими буквами** и непонятные ошибки
  Gradle или пакетов — перенесите кэши в путь из латинских букв и
  соберите заново:
  ```powershell
  [Environment]::SetEnvironmentVariable("PUB_CACHE", "C:\src\.pub-cache", "User")
  [Environment]::SetEnvironmentVariable("GRADLE_USER_HOME", "C:\src\.gradle", "User")
  ```
- **После обновления Flutter или кода** что-то сломалось — удалите
  созданные каталоги платформ и подготовьте проект заново (в них нет
  ничего своего, всё берётся из `tool\platform`):
  ```powershell
  git pull
  Remove-Item -Recurse -Force android, ios, macos, windows, linux, build
  python tool\bootstrap.py
  ```

## 7. Тесты

```powershell
cd rust; cargo test; cd ..   # ядро: соединения, сопряжение, передачи (около минуты)
flutter test                 # интерфейс: тексты, форматы, виджеты
```

---

## Build quick reference (English)

Requirements: Git, Python 3.10+, Rust via rustup, Flutter stable (tested
with 3.47), Visual Studio 2022 with *Desktop development with C++*
(Windows builds), Android Studio with *Android SDK Command-line Tools*
(Android builds; the NDK is installed automatically). On Windows enable
Developer Mode (`start ms-settings:developers`).

```powershell
git clone https://github.com/oedante/OgnelissShare.git
cd OgnelissShare
python tool\bootstrap.py                     # platform folders, packages, translations
flutter build windows --release              # build\windows\x64\runner\Release\ogneliss_share.exe
flutter build apk --release --target-platform android-arm64   # build\app\outputs\flutter-apk\app-release.apk
flutter run --release                        # build, install and start on a connected phone
```

Allow the app on private networks when Windows Firewall asks. Prebuilt
test builds: Actions → latest green **CI** run → Artifacts.
