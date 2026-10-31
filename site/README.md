# Страницы Ogneliss Share на ogneliss.ru

| Адрес | Что | Файл в статике |
|---|---|---|
| `/share/`, `/share/ru/` | описание и скачивание | `share/index.html`, `ru/share/index.html` |
| `/share/privacy`, `/share/ru/privacy` | политика конфиденциальности (её адрес — в Google Play и App Store) | `share/privacy.html`, `ru/share/privacy.html` |
| `/share/join/`, `/share/ru/join/` | страница ссылок-приглашений | `share/join.html`, `ru/share/join.html` |

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
for u in share/ share/privacy share/join/ share/ru/ share/ru/privacy share/ru/join/ share/app-icon.png; do
  curl -s -o /dev/null -w "%{http_code} /$u\n" "https://ogneliss.ru/$u?lang=en"
done
```

Оформление — стили остальных страниц сайта (`style.css.txt`, копия из
генератора сайта); шапка ведёт на главную ogneliss.ru. Добавить Share в
меню и на главную сайта — в генераторе самого сайта.
