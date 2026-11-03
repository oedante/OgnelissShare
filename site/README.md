# Страницы Ogneliss Share на ogneliss.ru

| Адрес | Что | Файл в статике |
|---|---|---|
| `/share/`, `/share/ru/` | описание и скачивание | `share/index.html`, `ru/share/index.html` |
| `/share/privacy`, `/share/ru/privacy` | политика конфиденциальности (её адрес — в Google Play и App Store) | `share/privacy.html`, `ru/share/privacy.html` |
| `/share/join/`, `/share/ru/join/` | страница ссылок-приглашений | `share/join.html`, `ru/share/join.html` |
| `/share/get/`, `/share/ru/get/` | получатель файлов в браузере (`…/get/#<комната>.<ключ>`, WebRTC; `docs/WEB_P2P.md`) | `share/get.html`, `ru/share/get.html` |

Собрать (нужен Pillow):

```bash
python3 site/build_share_site.py      # → site/out/
```

Выложить: содержимое `site/out/` скопировать в корень контейнера статики
сайта (`dialer-static`, тот же, что для остальных страниц ogneliss.ru), без
удаления чужих файлов:

```bash
STATIC=$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/usr/share/nginx/html"}}{{.Source}}{{end}}{{end}}' dialer-static)
sudo cp -r site/out/. "$STATIC"/
```

Один раз — правила `site/nginx-share.conf` в Nginx Proxy Manager (хост
ogneliss.ru → Advanced), рядом с разделом NumTouch. Без них адреса /share/…
переадресуются на главную.

Проверка:

```bash
for u in share/ share/privacy share/join/ share/get/ share/ru/ share/ru/privacy share/ru/join/ share/ru/get/ share/app-icon.png; do
  curl -s -o /dev/null -w "%{http_code} /$u\n" "https://ogneliss.ru/$u?lang=en"
done
```

Оформление — стили остальных страниц сайта (`style.css.txt`, копия из
генератора сайта); шапка ведёт на главную ogneliss.ru. Добавить Share в
меню и на главную сайта — в генераторе самого сайта.

Страница получателя `/share/get/` — единственная со скриптом: `site/get.js` (скрипт) и
`site/get.css.txt` (стили сверх общих) генератор вставляет прямо в страницу, внешних скриптов
нет, обращается она только к `/share/api/v1` (служба связи, `server/share-signal/`) и к STUN. Для
проверки на своём компьютере адрес службы можно заменить параметром `?api=http://localhost:…/share/api/v1`
(работает только на localhost). Сверх `docs/WEB_P2P.md` страница шлёт в канал `{"t":"ready"}`, если за секунду после открытия канала не пришло `hello` (Chromium теряет сообщение, посланное в тот же миг, как канал открылся); приложению достаточно в ответ повторить `hello`. Автотест — страница, поддельный отправитель и поддельная служба
связи в Chromium:

```bash
node site/test/get_page_test.mjs      # нужны Node 20+ и playwright; снимки экрана — в $SHOT_DIR (по умолчанию /tmp/ogneliss-get-shots)
```
