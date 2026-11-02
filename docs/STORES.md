# Ogneliss Share: выпуск в Google Play и App Store — по шагам

Короткая пошаговая инструкция: что сделать один раз и что — при каждом
выпуске. Подробности, ответы на анкеты магазинов и Microsoft Store — в
[RELEASE.md](RELEASE.md); установка Android Studio и Xcode — в
[BUILD_IDE.md](BUILD_IDE.md).

Перед каждым выпуском:
1. Поднять `version` в `pubspec.yaml` (например, `1.2.0+6` → `1.3.0+7`).
2. Написать заметки `store/whats_new/<версия>.txt` (по-русски и
   по-английски, не длиннее 500 знаков каждая) и проверить:
   `python tool/release/check_store.py`.
3. Отправить изменения в `main`. GitHub сам соберёт все платформы и, если
   всё собралось, опубликует выпуск `v<версия>` в Releases.

---

## 1. Секреты GitHub (один раз)

**Где:** github.com/oedante/OgnelissShare → **Settings** → **Secrets and
variables** → **Actions** → вкладка **Secrets** → **New repository secret**.
Имя — точно как в таблице. Значение вставляется целиком, без кавычек и
лишних пробелов. Сохранённое значение больше не показывается, его можно
только заменить (Update).

| Имя | Значение |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | файл ключа загрузки `upload-keystore.jks`, переведённый в текст base64 (команды ниже) |
| `ANDROID_KEYSTORE_PASSWORD` | пароль хранилища ключей (`storePassword`) |
| `ANDROID_KEY_ALIAS` | имя ключа (`keyAlias`), обычно `upload` |
| `ANDROID_KEY_PASSWORD` | пароль ключа (`keyPassword`) |
| `PLAY_SERVICE_ACCOUNT_JSON` | весь текст JSON-файла ключа сервисного аккаунта Google, от `{` до `}` |

**Ключ загрузки** (если его ещё нет) — один раз, в терминале, где установлена
Java (подойдёт терминал Android Studio):
```bash
keytool -genkey -v -keystore upload-keystore.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload
```
Сохраните файл и пароли в надёжном месте (менеджер паролей и копия на
другом носителе): без них новую версию в Google Play не выпустить.

**Файл ключа в base64:**
- Windows (PowerShell):
  `[Convert]::ToBase64String([IO.File]::ReadAllBytes("upload-keystore.jks")) | Set-Clipboard`
  — текст сразу в буфере обмена, вставьте его в поле секрета;
- macOS: `base64 -i upload-keystore.jks | pbcopy`;
- Linux: `base64 -w0 upload-keystore.jks`.

**JSON сервисного аккаунта Google** (для загрузки в Google Play из GitHub):
1. console.cloud.google.com → выберите или создайте проект →
   **APIs & Services → Library** → «Google Play Android Developer API» →
   **Enable**.
2. **IAM & Admin → Service Accounts → Create service account** → имя,
   например `ogneliss-play`; роли в Google Cloud не нужны → Done.
3. Откройте аккаунт → **Keys → Add key → Create new key → JSON** — файл
   скачается. Его содержимое — значение `PLAY_SERVICE_ACCOUNT_JSON`. Сам
   файл в репозиторий не кладите.
4. **Play Console → Пользователи и разрешения → Пригласить пользователя** →
   адрес аккаунта (`…@….iam.gserviceaccount.com`) → **Разрешения приложения**
   → Ogneliss Share: «Выпуск приложений в тестовые версии» и «Управление
   карточкой приложения» → Пригласить. Права начинают действовать не сразу
   (иногда до суток).

Проверка: Actions → **Google Play** → Run workflow. Если какого-то секрета
нет, запуск об этом напишет. Без `PLAY_SERVICE_ACCOUNT_JSON` он соберёт
подписанный AAB (Artifacts → `google-play`), но не загрузит.

Для iPhone и Mac секреты GitHub не нужны: ключи хранятся в Codemagic
(раздел 3).

---

## 2. Google Play

### Один раз

1. **Play Console → Создать приложение**: название «Ogneliss Share», язык
   по умолчанию — русский, «Приложение», «Бесплатно», согласиться с правилами.
2. **Первую сборку — вручную** (без неё Google Play не знает приложения, и
   загрузка из GitHub не работает):
   - возьмите подписанный AAB: Actions → **Google Play** → Run workflow →
     после завершения внизу страницы запуска **Artifacts → google-play** →
     внутри `ogneliss-share-<версия>-android.aab`;
     или соберите сами: `python tool/release/build.py android` (файл
     `android/key.properties` с тем же ключом — [RELEASE.md](RELEASE.md), раздел 3);
   - Play Console → **Тестирование → Внутреннее тестирование → Создать
     выпуск** → согласитесь на подпись приложений в Google Play → загрузите
     AAB → заметки из `store/whats_new/<версия>.txt` (вставляются как есть) →
     Сохранить → Проверить → **Начать выпуск**;
   - вкладка **Тестировщики** → список адресов → скопируйте ссылку
     «Присоединиться» и отправьте тестировщикам.
3. **Карточка приложения** (Развитие → Основная карточка): тексты из
   `store/play/ru-RU/` и `store/play/en-US/` (`title.txt`,
   `short_description.txt`, `full_description.txt`), значок
   `store/graphics/play_icon_512.png`, баннер
   `store/graphics/play_feature_1024x500.png`, 2–8 снимков экрана телефона.
4. **Содержание приложения** (политика конфиденциальности
   `https://ogneliss.ru/share/privacy`, реклама, доступ, рейтинг, аудитория,
   безопасность данных) — готовые ответы в [RELEASE.md](RELEASE.md), раздел 3.

### Каждый выпуск

- **Из GitHub:** Actions → **Google Play** → **Run workflow** → дорожка
  `internal` (внутреннее тестирование), `alpha` (закрытое) или `beta`
  (открытое) → Run. Сборка с заметками уйдёт на выбранную дорожку сама.
  Флажок «Обновить и карточку» заодно загружает тексты и картинки из `store/`.
- **С компьютера:**
  ```bash
  pip install google-api-python-client google-auth
  python tool/release/build.py android
  python tool/release/play.py dist/ogneliss-share-<версия>-android.aab --key play-key.json
  ```

Новый личный аккаунт разработчика: для выпуска «для всех» нужен закрытый тест
(дорожка `alpha`) — не меньше 12 тестировщиков 14 дней подряд. Внутреннее
тестирование доступно сразу.

---

## 3. App Store и TestFlight (iPhone, iPad, Mac)

### Один раз

1. **Apple Developer** (99 USD в год) → Certificates, Identifiers & Profiles →
   **Identifiers → «+» → App IDs → App** → Bundle ID (Explicit)
   `com.ogneliss.share`, описание «Ogneliss Share». Возможности не отмечать.
2. **App Store Connect → Приложения → «+» → Новое приложение**: платформа
   iOS, имя «Ogneliss Share», основной язык — русский, Bundle ID
   `com.ogneliss.share`, SKU `ogneliss-share`, полный доступ. Затем в этом же
   приложении слева **«+» рядом с iOS → macOS** — версия для Mac в той же
   записи.
3. **Ключ App Store Connect API** — если для Ogneliss Mail он уже добавлен в
   Codemagic под именем **Ogneliss ASC**, этот пункт пропустите. Иначе:
   App Store Connect → Пользователи и доступ → **Интеграции → App Store
   Connect API → «+»** → роль App Manager → скачать `AuthKey_XXXX.p8`
   (скачивается один раз), записать Key ID и Issuer ID → codemagic.io →
   **Teams → Integrations → App Store Connect → Add key** → имя
   `Ogneliss ASC`, Issuer ID, Key ID, файл `.p8`.
4. **Codemagic → Add application** → GitHub → `oedante/OgnelissShare` →
   тип проекта Flutter, конфигурация из `codemagic.yaml`.
5. **Только для Mac:** Codemagic → приложение → **Environment variables** →
   группа `ogneliss_share` → переменная `CERTIFICATE_PRIVATE_KEY`, отметить
   **Secret** → значение — содержимое файла `mac_key` целиком (со строками
   `-----BEGIN RSA PRIVATE KEY-----` и `-----END …`). Файл создаётся на любом
   компьютере с OpenSSH (Windows 10/11, macOS, Linux):
   ```bash
   ssh-keygen -t rsa -b 2048 -m PEM -f mac_key -q -N ""
   ```
   Храните `mac_key` у себя: тем же ключом Codemagic подписывает следующие
   сборки.
6. **TestFlight → Информация о тестировании**: описание из
   `store/testflight/ru/description.txt`, адрес для отзывов
   `oedante@gmail.com`, политика `https://ogneliss.ru/share/privacy`.
   **Внутреннее тестирование → «+»** — группа, добавьте себя и тестировщиков
   (до 100 участников команды App Store Connect).

### Каждый выпуск

1. codemagic.io → Ogneliss Share → **Start new build** → ветка `main` →
   workflow **iOS → TestFlight** (затем то же для **macOS → TestFlight**) →
   Start build. Сборка длится 15–30 минут; подпись Codemagic делает сам,
   номер сборки растёт сам.
2. Через 5–30 минут после окончания сборка появится в **App Store Connect →
   TestFlight**. Внутренние тестировщики получают её сразу.
3. Внешние тестировщики (по ссылке, до 10 000): TestFlight → **Внешнее
   тестирование → «+»** → группа → добавить сборку → «Что тестировать» из
   `store/testflight/ru/what_to_test.txt` → отправить на бета-проверку
   (обычно до суток).
4. **В App Store:** вкладка **Распространение** → версия (номер как в
   `pubspec.yaml`) → тексты из `store/appstore/ru/` и `en-US/`, снимки экрана
   (iPhone 6,9″ 1320×2868, iPad 13″ 2064×2752, Mac 2880×1800), сборка из
   TestFlight → «Конфиденциальность приложения», возрастной рейтинг,
   заметки для проверки ([RELEASE.md](RELEASE.md), раздел 4) → **Отправить
   на проверку**. Францию в «Цены и доступность» снимите, пока нет декларации
   ANSSI о шифровании.

**Есть свой Mac** (Xcode 26 или новее) — можно без Codemagic:
```bash
export APPLE_TEAM_ID=ABCDE12345          # developer.apple.com → Membership details
tool/release/testflight.sh all           # iPhone/iPad и Mac, вход — из Xcode → Settings → Accounts
```
или в Xcode: Product → Archive → Distribute App → App Store Connect → Upload
([BUILD_IDE.md](BUILD_IDE.md), раздел 2).

---

## 4. Если что-то пошло не так

- **GitHub: красный крестик в Actions** — откройте запуск, затем шаг с
  ошибкой; или ветка `ci-status` репозитория: `SUMMARY.txt` и журналы
  `log-*/…errors.txt`.
- **«Google Play»: `Package not found`** — первая сборка ещё не загружена
  вручную (раздел 2). **`The caller does not have permission`** — сервисному
  аккаунту не выданы права в Play Console или они ещё не вступили в силу.
  **`version code … already been used`** — эта сборка уже загружена,
  запустите заново (номер растёт сам).
- **Codemagic: `No matching profiles` / ошибки подписи** — проверьте, что
  Bundle ID `com.ogneliss.share` создан (раздел 3, шаг 1), а ключ API
  добавлен в Codemagic с ролью App Manager.
- **App Store Connect не принимает сборку** — нужен Xcode 26 или новее
  (в Codemagic стоит `xcode: latest`); при повторной загрузке того же номера
  сборки — запустите сборку заново.
