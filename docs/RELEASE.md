# Сборка под все устройства и выпуск в магазины

Подробная установка программ для Windows и Android — в [BUILD.md](BUILD.md).
Здесь: короткий порядок сборки для каждой платформы, macOS и iOS, и выпуск
в тест в Google Play, App Store (TestFlight) и Microsoft Store.

> **Без своей сборки.** При каждом изменении `main` GitHub собирает все
> платформы. Если в `pubspec.yaml` новая версия, он сам публикует выпуск в
> [Releases](https://github.com/oedante/OgnelissShare/releases). Файлы для
> магазинов лежат в Actions → запуск → Artifacts:
> `store-android-aab` (Google Play) и `store-windows-msix` (Microsoft Store).

## 1. Общее для всех платформ

Нужно: Git, Python 3.10+, Rust (rustup), Flutter stable (проверено на 3.47).

```bash
git clone https://github.com/oedante/OgnelissShare.git
cd OgnelissShare
python3 tool/bootstrap.py        # каталоги платформ, пакеты, переводы
```

Сборка для выпуска — одной командой, готовые файлы появляются в `dist/`:

| Платформа | Где собирать | Команда | Результат |
|---|---|---|---|
| Android | Windows, macOS, Linux | `python tool/release/build.py android` | AAB для Google Play и APK |
| Windows | Windows | `python tool/release/build.py windows --msix` | zip и MSIX для Microsoft Store |
| Linux | Linux | `python tool/release/build.py linux` | tar.gz |
| macOS | Mac | `python3 tool/release/build.py macos` | zip с .app без подписи — для проверки |
| iOS | Mac | `python3 tool/release/build.py ios` | .ipa без подписи — для проверки |

Версия берётся из `pubspec.yaml` (`version: 0.1.1+2`). Номер сборки для
Android, iOS и macOS считается сам: это минуты с 01.01.2026, поэтому каждая
следующая сборка больше предыдущей. Перед выпуском поднимите `version` и
допишите заметки `store/whats_new/<версия>.txt`.

Проверки перед выпуском:
`python tool/release/check_store.py` (длина текстов магазинов) и
`python tool/release/check_apk.py dist/*.aab` (страницы памяти 16 КБ: без
них Google Play сборку не примет). Сборка Android проверяет 16 КБ сама.

## 2. macOS и iOS: сборка на Mac

1. Mac с **Xcode 26 или новее**: с 28.04.2026 App Store Connect принимает
   только такие сборки. Также CocoaPods (`brew install cocoapods`), Flutter,
   Rust и цели Rust:
   `rustup target add aarch64-apple-ios aarch64-apple-darwin x86_64-apple-darwin`.
2. Подготовка проекта: `python3 tool/bootstrap.py`. Скрипт сам добавит
   описание приватности (`PrivacyInfo.xcprivacy`), переводы, права песочницы
   macOS и ключ `ITSAppUsesNonExemptEncryption`.
3. Запуск на своём iPhone: `open ios/Runner.xcworkspace` → Runner →
   Signing & Capabilities → Team, затем `flutter run --release`.
   На Mac: `flutter run -d macos`.

Без своего Mac iOS и macOS собирает облако Codemagic (раздел 4).

## 3. Google Play

### Один раз

1. **Ключ загрузки.** Он подписывает каждую сборку. Сделайте копию в
   надёжном месте: без ключа новую версию не загрузить, восстановить его
   можно только через поддержку Google.
   ```bash
   keytool -genkey -v -keystore upload-keystore.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload
   ```
   Файл `android/key.properties` (в git не попадает):
   ```
   storeFile=/полный/путь/upload-keystore.jks
   storePassword=…
   keyAlias=upload
   keyPassword=…
   ```
   Для сборок на GitHub те же данные кладутся в секреты репозитория
   (Settings → Secrets and variables → Actions):
   `ANDROID_KEYSTORE_BASE64` (вывод `base64 -w0 upload-keystore.jks`),
   `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
2. **Приложение в Play Console:** «Создать приложение» → название Ogneliss
   Share, язык по умолчанию — русский, «Приложение», «Бесплатно».
   Идентификатор пакета — `com.ogneliss.share` (он задаётся первой
   загруженной сборкой).
3. **Первая сборка — вручную:** `python tool/release/build.py android` →
   Play Console → Тестирование → **Внутреннее тестирование** → Создать
   выпуск → загрузить `dist/ogneliss-share-<версия>-android.aab`.
   Подпишитесь на Play App Signing (Google хранит ключ приложения, вы —
   только ключ загрузки). Добавьте тестировщиков списком адресов и
   отправьте им ссылку «Присоединиться к тестированию».
4. **Сервисный аккаунт для загрузки скриптом.** Google Cloud Console →
   проект → включить «Google Play Android Developer API» → IAM →
   Сервисные аккаунты → создать → Ключи → JSON. Затем в Play Console →
   Пользователи и разрешения → пригласить адрес сервисного аккаунта и дать
   права на приложение: «Выпуски в тестовые версии», «Карточка приложения».
   Файл ключа не кладите в репозиторий. Для GitHub его содержимое идёт в
   секрет `PLAY_SERVICE_ACCOUNT_JSON`.

### Каждый выпуск

С компьютера:
```bash
pip install google-api-python-client google-auth
python tool/release/build.py android
python tool/release/play.py dist/ogneliss-share-<версия>-android.aab --key play-key.json
```
Или в GitHub: Actions → **Google Play** → Run workflow (дорожка
`internal`). `--track alpha` — закрытое тестирование, `beta` — открытое.
`--listing --images` заодно обновляет тексты из `store/play/` и картинки
из `store/graphics/`. `--check` проверяет доступ, ничего не меняя.

### Что заполнить в Play Console

- **Карточка:** тексты — `store/play/ru-RU/` и `en-US/`, значок —
  `store/graphics/play_icon_512.png`, баннер — `play_feature_1024x500.png`.
  Снимки экрана (минимум 2) снимаются на телефоне.
- **Политика конфиденциальности:** `https://ogneliss.ru/share/privacy`.
  Страницу сначала нужно выложить на сайт.
- **Категория:** Инструменты. **Реклама:** нет. **Доступ к приложению:** всё
  доступно без входа. Для проверки нужны два устройства, иначе используйте
  «Файлы по ссылке».
- **Целевая аудитория:** 13 лет и старше, без групп младше 13.
- **Возрастной рейтинг (IARC):** категория «Утилиты». На вопрос об обмене
  контентом между пользователями ответьте «да»: файлы можно передать
  другому человеку.
- **Безопасность данных:** файлы, тексты и имена устройств зашифрованы
  сквозным шифрованием, поэтому по правилам Google не считаются собранными.
  Честнее указать одно: «Идентификаторы устройства или другие» — собираются
  (открытый ключ устройства публикуется в службе адресов n0, чтобы
  устройства находили друг друга), цель — функции приложения, не передаются
  третьим лицам, шифруются при передаче. Это моё понимание, а не
  юридическая консультация.
- **Новый личный аккаунт разработчика** (создан после ноября 2023): перед
  публикацией для всех нужен закрытый тест — не меньше 12 тестировщиков в
  течение 14 дней подряд. Внутреннее тестирование доступно сразу.
- С 31.08.2026 Google Play принимает только сборки с целевым API 36. CI
  печатает целевой API каждой сборки (`targetSdkVersion` в журнале Android).

### Ссылки-приглашения открываются сразу в приложении (Android App Links)

После первой загрузки возьмите отпечатки SHA-256 в Play Console → Проверка
целостности приложения → Подпись приложения. Нужны оба: ключа подписи
Google и ключа загрузки. Положите на сайт файл
`https://ogneliss.ru/.well-known/assetlinks.json`:
```json
[{"relation": ["delegate_permission/common.handle_all_urls"],
  "target": {"namespace": "android_app", "package_name": "com.ogneliss.share",
             "sha256_cert_fingerprints": ["AA:BB:…", "CC:DD:…"]}}]
```
Без этого файла ссылка открывает страницу сайта, а приложение — через
вставку ссылки.

APK из GitHub Releases подписан ключом загрузки, а приложение из Google Play
— ключом Google. Поверх друг друга они не ставятся: перед установкой другой
версии удалите прежнюю.

## 4. App Store и TestFlight (iPhone, iPad, Mac)

### Один раз

1. Программа Apple Developer (99 USD в год).
2. **Идентификатор:** Certificates, Identifiers & Profiles → Identifiers →
   App ID `com.ogneliss.share`. Дополнительные возможности не нужны.
3. **App Store Connect** → Приложения → «+» → Новое приложение: iOS, имя
   «Ogneliss Share», основной язык — русский, Bundle ID `com.ogneliss.share`,
   SKU `ogneliss-share`. Версию для Mac добавьте в ту же запись:
   «Добавить платформу» → macOS. Идентификатор тот же, покупка общая.
4. **Ключ App Store Connect API:** Пользователи и доступ → Интеграции →
   App Store Connect API → «+». Роль App Manager подходит для Codemagic;
   для `testflight.sh` на Mac без входа в Xcode нужна роль Admin (Xcode
   сам создаёт сертификаты). Скачайте `AuthKey_XXXX.p8` (он скачивается один
   раз) и запишите Key ID и Issuer ID.

### Сборка и загрузка: вариант А — Codemagic (без своего Mac)

Так же, как у Ogneliss Mail:
1. Подключите репозиторий в codemagic.io. Ключ API: Teams → Integrations →
   App Store Connect → имя **Ogneliss ASC** (тот же, что у Mail).
2. Только для Mac: Environment variables → группа `ogneliss_share` →
   `CERTIFICATE_PRIVATE_KEY` (Secret). Это содержимое файла из
   `ssh-keygen -t rsa -b 2048 -m PEM -f mac_key -q -N ""`.
3. Start new build → **iOS → TestFlight** или **macOS → TestFlight**
   (`codemagic.yaml`). Подпись Codemagic настроит сам, сборка уйдёт в
   App Store Connect.

### Вариант Б — на своём Mac

```bash
export APPLE_TEAM_ID=ABCDE12345                 # developer.apple.com → Membership
export ASC_KEY_ID=… ASC_ISSUER_ID=… ASC_KEY_PATH=~/keys/AuthKey_….p8   # или вход в Xcode
tool/release/testflight.sh all                  # ios, macos или all
```
С `UPLOAD=0` скрипт только соберёт подписанные `.ipa` и `.pkg` в `dist/`.
Их можно загрузить программой Transporter.

### TestFlight

- Сборка появляется в App Store Connect → TestFlight через 5–30 минут.
- **Внутренние тестировщики** (до 100 участников вашей команды) получают
  сборку сразу, без проверки Apple.
- **Внешние** (до 10 000, по ссылке или адресам) — после короткой
  бета-проверки. Нужна «Информация о тестировании»: тексты из
  `store/testflight/ru/` и `en-US/`, адрес для отзывов, политика
  конфиденциальности.

### Что заполнить в App Store Connect

- **Тексты:** `store/appstore/ru/` и `en-US/` — название, подзаголовок,
  рекламный текст, описание, ключевые слова, адреса поддержки и политики.
  Категории: «Утилиты», вторая — «Производительность». Copyright:
  `© 2026 Ermishin Oleg`.
- **Значок** берётся из сборки. **Снимки экрана:** iPhone 6,9″
  (1320×2868), iPad 13″ (2064×2752), Mac (2880×1800). Для TestFlight они не
  нужны, для App Store обязательны.
- **Конфиденциальность приложения:** «Данные не собираются». Файлы и тексты
  зашифрованы сквозным шифрованием и идут напрямую; ретрансляторы и служба
  адресов обрабатывают только зашифрованный поток и открытый ключ
  устройства, в реальном времени. Если хотите указать осторожнее —
  «Идентификаторы → ID устройства: функции приложения, не связано с
  личностью, без отслеживания». Отслеживания (ATT) нет.
- **Возрастной рейтинг:** все ответы «нет» → 4+.
- **Экспортные правила (шифрование).** Приложение использует стандартные
  алгоритмы (TLS 1.3, QUIC) из своей библиотеки, а не из iOS. По правилам
  Apple документы для этого нужны только для App Store во Франции
  (декларация ANSSI). Поэтому в Info.plist стоит
  `ITSAppUsesNonExemptEncryption = NO`, и вопрос у каждой сборки не
  появляется. Пока декларации нет, снимите Францию в «Цены и доступность».
  Если подадите декларацию — поменяйте ключ на YES в `tool/bootstrap.py`
  (`ENCRYPTION_KEYS`) и добавьте код, который выдаст Apple. Это не
  юридическая консультация.
- **Заметки для проверки (App Review), по-английски:**
  ```
  Ogneliss Share transfers files directly between the user's own devices
  (end-to-end encrypted, no accounts, no server storage). To test, install
  the app on two devices: on one tap "Add a device" to show a QR code, on
  the other tap "Scan code". Alternatively use "Files by link" on one
  device and open the link on the other. The camera is used only to scan
  pairing QR codes; local network access finds the user's devices on the
  same Wi-Fi.
  ```

## 5. Microsoft Store (Windows)

Регистрация частного разработчика бесплатна; нужна проверка документа.

1. Partner Center → Приложения и игры → Новый продукт → «Приложение MSIX
   или PWA» → зарезервировать «Ogneliss Share».
2. Управление продуктом → **Удостоверение продукта** → перенесите три
   значения в `store/windows/identity.json` (Package/Identity/Name,
   Publisher, PublisherDisplayName) и удалите строку `"test": true`.
3. `python tool/release/build.py windows --msix` →
   `dist/ogneliss-share-<версия>-windows-x64.msix`. Подписывать не нужно:
   это сделает Store. Номер версии пакета — `X.Y.Z.0`, при каждой загрузке
   он должен расти.
4. Новая заявка → **Цены и доступность → Видимость → Закрытая аудитория**:
   группа тестировщиков по адресам учётных записей Microsoft. Так
   приложение видят только они.
5. Пакеты → загрузить .msix. В «Параметрах отправки» объясните
   `runFullTrust`: *Win32 desktop app (Flutter) packaged as MSIX; needs full
   trust to run its own executable and open direct network connections
   between the user's devices.* Описание — из `store/play/…/full_description.txt`,
   политика — `https://ogneliss.ru/share/privacy`, минимум один снимок экрана
   1366×768 или больше.
6. Проверка занимает до трёх рабочих дней.

Проверить пакет у себя до Store (нужен режим разработчика Windows):
`Add-AppxPackage -Register build\msix\layout\AppxManifest.xml`.

## 6. Где что лежит

| Файл | Что это |
|---|---|
| `tool/release/build.py` | сборка всех платформ в `dist/` |
| `tool/release/play.py` | загрузка в Google Play |
| `tool/release/testflight.sh` | TestFlight с Mac (iOS и macOS) |
| `codemagic.yaml` | TestFlight из облака Codemagic |
| `.github/workflows/ci.yml` | проверка, сборки, выпуски в GitHub Releases |
| `.github/workflows/store.yml` | загрузка в Google Play из GitHub |
| `tool/release/check_store.py`, `check_apk.py` | проверки перед выпуском |
| `store/` | тексты магазинов, заметки к версиям, картинки, имя пакета MSIX |
| `tool/branding/make_store_graphics.py` | значок 512 и баннер для Google Play |

Ключи (`*.jks`, `*.p8`, JSON сервисного аккаунта, `key.properties`) в git
не попадают — они указаны в `.gitignore`. Храните копии отдельно.
