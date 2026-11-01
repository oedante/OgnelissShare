# Сборка на своём компьютере: Android Studio, Xcode и секреты GitHub

Главное правило проекта: каталоги `android/`, `ios/`, `macos/`, `windows/`,
`linux/` в git не хранятся. Их создаёт `tool/bootstrap.py` из шаблона Flutter
и накладки `tool/platform/`. Поэтому:

- после `git clone` и после каждого `git pull` или обновления Flutter
  сначала выполните `python tool/bootstrap.py`, и только потом открывайте
  проект в Android Studio или Xcode;
- свои правки в этих каталогах не сохраняются: постоянные изменения вносятся
  в `tool/platform/` или в `tool/bootstrap.py`.

Ядро на Rust собирается само во время сборки Flutter (Cargokit). Rust он
находит сам в `~/.cargo/bin`, в том числе при сборке из Xcode.

---

## 1. Android в Android Studio (Windows, macOS, Linux)

### Что установить один раз

1. **Android Studio** — https://developer.android.com/studio. При первом
   запуске мастер поставит Android SDK.
2. **SDK Manager** (More Actions → SDK Manager → SDK Tools): отметьте
   *Android SDK Command-line Tools (latest)*. NDK нужной версии сборка
   поставит сама; если она пишет об отсутствии NDK — отметьте там же
   *NDK (Side by side)* и *CMake*.
3. **Flutter** (stable) — https://docs.flutter.dev/get-started/install,
   папку `flutter/bin` добавьте в PATH. На Windows — путь без пробелов и
   русских букв, например `C:\src\flutter`.
4. **Плагины Android Studio**: Settings → Plugins → Marketplace → *Flutter*
   (Dart поставится вместе с ним). Перезапустите Android Studio.
5. **Rust** — https://rustup.rs (ответы по умолчанию). На Windows ещё
   Visual Studio 2022 с нагрузкой «Разработка классических приложений на
   C++» (подробно — [BUILD.md](BUILD.md)).
6. **Python 3.10+**.
7. В терминале: `flutter doctor --android-licenses` (принять лицензии), затем
   `flutter doctor` — строка *Android toolchain* должна быть без ошибок.

### Открыть и запустить

```bash
git clone https://github.com/oedante/OgnelissShare.git
cd OgnelissShare
python tool/bootstrap.py
```

1. Android Studio → **File → Open** → папка `OgnelissShare` целиком (не
   `android/` внутри неё). Студия узнает проект Flutter.
2. Settings → Languages & Frameworks → **Flutter** → Flutter SDK path —
   папка Flutter, если не подхватилась сама.
3. Settings → Build, Execution, Deployment → Build Tools → Gradle →
   **Gradle JDK**: встроенный JBR 21 (*jbr-21*).
4. Телефон: «Параметры разработчика» → «Отладка по USB» (параметры
   разработчика открываются семью нажатиями на «Номер сборки»). Подключите
   кабель и разрешите отладку на телефоне. Или создайте эмулятор: Device
   Manager → «+».
5. Вверху выберите устройство и конфигурацию **main.dart** → ▶ **Run**.
   Первая сборка — 10–20 минут (ядро Rust), следующие быстрее.
6. Сборка для проверки скорости и выпуска: Run → Edit Configurations →
   main.dart → *Additional run args*: `--release`.

Журнал: окно **Logcat**, фильтр `OgnelissShare` (Android) и окно **Run**
(Flutter). В самом приложении — Настройки → Журнал.

### Сборка для Google Play

1. Один раз — ключ загрузки и `android/key.properties` (команды — в
   [RELEASE.md](RELEASE.md), раздел 3). **Храните копию ключа отдельно:**
   без него новую версию в Google Play не выпустить.
2. В терминале Android Studio (внизу, вкладка Terminal):
   ```bash
   python tool/release/build.py android
   ```
   В `dist/` появятся `ogneliss-share-<версия>-android.aab` для Google Play и
   APK для установки напрямую. Скрипт сам проверит требование Google Play о
   страницах памяти 16 КБ.
   То же из меню: Build → Flutter → Build App Bundle (файл —
   `build/app/outputs/bundle/release/app-release.aab`), но номер сборки
   тогда будет из `pubspec.yaml`, и для второй загрузки его нужно поднимать
   вручную.
3. Загрузка — вручную в Play Console или `tool/release/play.py`
   ([RELEASE.md](RELEASE.md), раздел 3).

### Если не собирается

- **Ошибки NDK или CMake** — SDK Manager → SDK Tools → NDK (Side by side) и
  CMake; `flutter doctor --android-licenses`.
- **«Unsupported class file major version» / ошибки Java** — Gradle JDK =
  jbr-21 (пункт 3).
- **После обновления Flutter или `git pull` что-то сломалось** — удалите
  `android/` и `build/`, снова `python tool/bootstrap.py`, затем File →
  Invalidate Caches → Invalidate and Restart.
- Windows: длинные пути и русские буквы в имени пользователя — [BUILD.md](BUILD.md), раздел 6.

---

## 2. iOS и macOS в Xcode и терминале (только Mac)

### Что установить один раз

1. **Xcode 26 или новее** из App Store (App Store Connect с 28.04.2026 берёт
   только такие сборки). Запустите один раз, затем в терминале:
   ```bash
   sudo xcodebuild -license accept
   xcodebuild -runFirstLaunch
   ```
   Xcode → Settings → Components — скачайте платформу **iOS** (для
   симулятора и телефона).
2. **Homebrew** (https://brew.sh), затем `brew install cocoapods python`.
3. **Flutter** (stable): распакуйте в `~/development/flutter` и добавьте в
   `~/.zshrc`: `export PATH="$HOME/development/flutter/bin:$PATH"`.
4. **Rust** и его цели для Apple:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios \
                     aarch64-apple-darwin x86_64-apple-darwin
   ```
5. `flutter doctor` — строка *Xcode* без ошибок.

### Подготовить проект

```bash
git clone https://github.com/oedante/OgnelissShare.git
cd OgnelissShare
python3 tool/bootstrap.py
flutter build ios --config-only --no-codesign     # пакеты CocoaPods и настройки для Xcode
flutter build macos --config-only
```

Открывайте **`.xcworkspace`, а не `.xcodeproj`**: `open ios/Runner.xcworkspace`
или `open macos/Runner.xcworkspace`.

### iPhone и iPad

1. В Xcode: слева **Runner** → цель Runner → **Signing & Capabilities** →
   *Automatically manage signing* → **Team** — ваша команда Apple
   Developer (если её нет: Xcode → Settings → Accounts → «+» → Apple ID).
   - С **бесплатным Apple ID** (без программы за 99 USD) приложение
     ставится только на свои устройства и работает 7 дней. Идентификатор
     `com.ogneliss.share` занят вашим платным аккаунтом. Если его нет,
     поменяйте Bundle Identifier на свой, например
     `com.ogneliss.share.dev`: каталог `ios/` всё равно не в git.
2. iPhone: подключите кабелем, «Доверять этому компьютеру». Включите
   *Настройки → Конфиденциальность и безопасность → Режим разработчика*
   (телефон перезагрузится).
3. Вверху выберите iPhone → ▶ (**⌘R**). При первом запуске разрешите
   доступ к локальной сети. С бесплатным Apple ID ещё: *Настройки →
   Основные → VPN и управление устройством → доверять разработчику*.

То же из терминала:
```bash
flutter devices                      # список устройств
flutter run --release -d <id>        # собрать, поставить и запустить
open -a Simulator && flutter run     # симулятор (камеры в нём нет — вставляйте ссылку)
```

### Mac

```bash
flutter run -d macos                       # отладка
python3 tool/release/build.py macos        # dist/…-macos.zip без подписи, для проверки
```
В Xcode: `open macos/Runner.xcworkspace` → Runner → Signing & Capabilities →
Team (или *Sign to Run Locally*) → ▶. Приложение без подписи на другом Mac
открывается правым щелчком → «Открыть».

### Выпуск в TestFlight из Xcode

Перед архивом в терминале задайте версию и номер сборки. Номер должен расти;
считайте его так же, как скрипты выпуска (минуты с 01.01.2026), иначе
следующая сборка скриптом окажется «меньше» и App Store Connect её отклонит:
```bash
BUILD=$(python3 -c 'import datetime as d; print(int((d.datetime.now(d.timezone.utc) - d.datetime(2026, 1, 1, tzinfo=d.timezone.utc)).total_seconds() // 60))')
flutter build ios --config-only --release --build-name=1.1.0 --build-number=$BUILD
```
1. Вверху устройство **Any iOS Device (arm64)** → **Product → Archive**.
2. Откроется **Organizer** → архив → **Distribute App** → **App Store
   Connect** → **Upload**. Подпись Xcode настроит сам.
3. Через 5–30 минут сборка появится в App Store Connect → TestFlight.

Для Mac — то же в `macos/Runner.xcworkspace`, сначала
`flutter build macos --config-only --release --build-name=1.1.0 --build-number=$BUILD`,
устройство **Any Mac**.

Без окон Xcode, одной командой, с проверкой версии Xcode и номером сборки:
`tool/release/testflight.sh ios|macos|all` ([RELEASE.md](RELEASE.md), раздел 4).

### Если не собирается

- **«CocoaPods not installed» / ошибки Pods** — `brew install cocoapods`,
  затем `cd ios && pod install --repo-update && cd ..`.
- **Ошибка в шаге сборки ядра Rust** (Xcode показывает только последнюю
  строку) — полный текст:
  `cd rust && cargo build --release --target aarch64-apple-ios`.
- **«No profiles for com.ogneliss.share»** — выберите Team; при бесплатном
  Apple ID поменяйте Bundle Identifier (выше).
- **После `git pull` или обновления Flutter** — `rm -rf ios macos build`,
  снова `python3 tool/bootstrap.py` и команды `--config-only`.

---

## 3. Секреты GitHub

Где: **GitHub → репозиторий OgnelissShare → Settings → Secrets and variables
→ Actions → вкладка Secrets → New repository secret**. Имя — ровно как в
таблице, значение — целиком, без кавычек. После сохранения значение больше
не показывается (только заменить).

| Секрет | Что вписать | Для чего |
|---|---|---|
| `ANDROID_KEYSTORE_BASE64` | файл ключа загрузки в base64: Windows PowerShell — `[Convert]::ToBase64String([IO.File]::ReadAllBytes("upload-keystore.jks"))`, Mac/Linux — `base64 -i upload-keystore.jks` (Linux: `base64 -w0 upload-keystore.jks`) | подпись APK в Releases и AAB для Google Play |
| `ANDROID_KEYSTORE_PASSWORD` | пароль хранилища ключей (storePassword) | то же |
| `ANDROID_KEY_ALIAS` | имя ключа (keyAlias), обычно `upload` | то же |
| `ANDROID_KEY_PASSWORD` | пароль ключа (keyPassword) | то же |
| `PLAY_SERVICE_ACCOUNT_JSON` | содержимое JSON-файла ключа сервисного аккаунта Google Cloud целиком, от `{` до `}` | загрузка в Google Play кнопкой Actions → «Google Play» |

Без четырёх `ANDROID_*` сборки подписываются отладочным ключом: для проверки
годятся, для Google Play — нет. Без `PLAY_SERVICE_ACCOUNT_JSON` запуск
«Google Play» только собирает подписанный AAB (Actions → запуск → Artifacts
→ `google-play`), а загружаете вы его сами. Первую сборку Google Play в
любом случае требует загрузить вручную.

Как получить JSON сервисного аккаунта:
1. https://console.cloud.google.com → создайте проект (или выберите) →
   APIs & Services → Library → **Google Play Android Developer API** → Enable.
2. IAM & Admin → **Service Accounts** → Create service account (роль не
   нужна) → откройте его → Keys → Add key → **JSON** — файл скачается.
3. **Play Console** → Пользователи и разрешения → Пригласить пользователя →
   адрес сервисного аккаунта (…@….iam.gserviceaccount.com) → права на
   приложение Ogneliss Share: «Выпуск приложений в тестовые версии»,
   «Управление карточкой приложения».

**Переменные (не секреты)** — та же страница, вкладка *Variables*:
`OGNELISS_RELAYS` — свои ретрансляторы через запятую. Сейчас не нужна.

**Для iOS и macOS секреты GitHub не нужны**: TestFlight собирается в
Codemagic. Там же и хранятся ключи:
- codemagic.io → Teams → Integrations → App Store Connect → ключ API с
  именем **Ogneliss ASC** (Issuer ID, Key ID и файл `.p8` из App Store
  Connect → Пользователи и доступ → Интеграции);
- приложение → Environment variables → группа **ogneliss_share** →
  `CERTIFICATE_PRIVATE_KEY` (Secret, только для Mac): содержимое файла
  из `ssh-keygen -t rsa -b 2048 -m PEM -f mac_key -q -N ""` целиком,
  со строками BEGIN и END.

Ключи и пароли в репозиторий не кладите: `*.jks`, `*.p8`, `key.properties`
и JSON сервисных аккаунтов уже исключены через `.gitignore`.
