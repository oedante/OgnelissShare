#!/usr/bin/env bash
# =============================================================================
#  Ogneliss Share — служба связи для ссылок «без приложения» (share-signal):
#  установка, обновление и проверка. Сервер с Docker и Nginx Proxy Manager —
#  тот же, где почтовый сервер и сайт (/opt/mailserver). Запуск от root.
#
#  Команды:
#    install   первая установка (по умолчанию); повторный запуск — как update
#    update    обновить код службы из этого архива и пересобрать контейнер
#    check     проверить: контейнер, ответ службы, адрес снаружи
#    remove    остановить и удалить контейнер (каталог службы остаётся)
#
#  Примеры:
#    sudo bash install-share-signal.sh install
#    sudo bash install-share-signal.sh update
#    sudo bash install-share-signal.sh check
#
#  Подробно: README.md рядом и docs/WEB_P2P.md (раздел 2).
#
#  Что хранит служба: ничего на диске. В памяти — зашифрованные описания
#  соединения (SDP) до часа; расшифровать их она не может, ключа у неё нет.
#  Журнала запросов нет, ни файлов, ни ключей служба не видит.
# =============================================================================
set -Eeuo pipefail
umask 077

SCRIPT_VERSION="2026.11.1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Префикс файловой системы — только для автотестов скрипта. Не задавайте его.
ROOT="${OG_ROOT:-}"

# ── Параметры (флаги перекрывают переменные окружения) ──────────────────────
CMD=""
MAIL_DIR="${MAIL_DIR:-/opt/mailserver}"
NPM_NETWORK="${NPM_NETWORK:-}"
BASE_URL_ARG="${BASE_URL:-}"
PORT=8789
CONTAINER=share-signal
IMAGE=ogneliss/share-signal:latest
COMPOSE_FILE=docker-compose.share.yml
DEFAULT_BASE_URL="https://ogneliss.ru/share/api/v1"

# ── Вывод ───────────────────────────────────────────────────────────────────
if [ -t 1 ]; then C_G=$'\e[32m'; C_Y=$'\e[33m'; C_R=$'\e[31m'; C_B=$'\e[1m'; C_0=$'\e[0m'
else C_G=""; C_Y=""; C_R=""; C_B=""; C_0=""; fi
log()  { printf '%s==>%s %s\n' "$C_B" "$C_0" "$*" >&2; }
ok()   { printf '  %s✓%s %s\n' "$C_G" "$C_0" "$*" >&2; }
warn() { printf '  %s!%s %s\n' "$C_Y" "$C_0" "$*" >&2; }
bad()  { printf '  %s✗%s %s\n' "$C_R" "$C_0" "$*" >&2; }
die()  { printf '%sОШИБКА:%s %s\n' "$C_R" "$C_0" "$*" >&2; exit 1; }
trap 'printf "%sОШИБКА%s в строке %s: %s\n" "$C_R" "$C_0" "$LINENO" "$BASH_COMMAND" >&2' ERR

usage() {
  sed -n '3,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  cat <<'EOT'
Флаги:
  --base-url URL        внешний адрес службы, по умолчанию
                        https://ogneliss.ru/share/api/v1
  --mail-dir DIR        каталог сервера, по умолчанию /opt/mailserver
  --network NAME        сеть Docker Nginx Proxy Manager (по умолчанию — общая
                        с контейнерами proxy-manager и oauth-proxy)
EOT
}

# ── Разбор аргументов ───────────────────────────────────────────────────────
parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      install|update|check|remove|help) CMD="$1" ;;
      -h|--help) CMD=help ;;
      --base-url) BASE_URL_ARG="${2:?}"; shift ;;
      --mail-dir) MAIL_DIR="${2:?}"; shift ;;
      --network) NPM_NETWORK="${2:?}"; shift ;;
      *) die "неизвестный параметр: $1 (см. --help)" ;;
    esac
    shift
  done
  [ -n "$CMD" ] || CMD=install
  APP_DIR="$ROOT$MAIL_DIR/share-signal"
}

# ── Утилиты ─────────────────────────────────────────────────────────────────
need_root() { [ -n "$ROOT" ] || [ "$(id -u)" -eq 0 ] || die "запустите от root: sudo bash $0 ${CMD}"; }
have() { command -v "$1" >/dev/null 2>&1; }

json_val() {  # json_val JSON NAME → значение поля (число, true/false или строка без кавычек)
  printf '%s' "$1" | sed -n "s/.*\"$2\":\(\"[^\"]*\"\|[^,}]*\).*/\1/p" | sed 's/^"//; s/"$//'
}

parse_base_url() {  # задаёт BASE_URL, BASE_HOST, BASE_PATH
  BASE_URL="${1%/}"
  if [[ "$BASE_URL" =~ ^https://([A-Za-z0-9.-]+)(/[A-Za-z0-9/_-]*)$ ]]; then
    BASE_HOST="${BASH_REMATCH[1]}"; BASE_PATH="${BASH_REMATCH[2]%/}"
  else
    die "адрес службы должен быть вида https://домен/путь (например $DEFAULT_BASE_URL), получено: $1"
  fi
  [ -n "$BASE_PATH" ] || die "в адресе нужен путь, например $DEFAULT_BASE_URL"
  # Служба отвечает только под /share/api/v1: другой путь она не знает.
  [ "$BASE_PATH" = /share/api/v1 ] || warn "путь $BASE_PATH не совпадает с /share/api/v1, на котором слушает служба — проверка снаружи не пройдёт"
}

resolve_base_url() { parse_base_url "${BASE_URL_ARG:-$DEFAULT_BASE_URL}"; }

compose_cmd() {
  if docker compose version >/dev/null 2>&1; then DC=(docker compose); DC_V2=1
  elif have docker-compose; then DC=(docker-compose); DC_V2=0
  else die "нет docker compose / docker-compose"; fi
}

# Сеть Nginx Proxy Manager: общая у proxy-manager и oauth-proxy, иначе первая
# сеть proxy-manager, иначе mailserver_default.
detect_network() {
  [ -n "$NPM_NETWORK" ] && return 0
  local npm_nets oauth_nets n
  npm_nets="$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' proxy-manager 2>/dev/null || true)"
  oauth_nets="$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' oauth-proxy 2>/dev/null || true)"
  for n in $npm_nets; do
    case " $oauth_nets " in *" $n "*) NPM_NETWORK="$n"; return 0 ;; esac
  done
  NPM_NETWORK="$(printf '%s' "$npm_nets" | awk '{print $1}')"
  [ -n "$NPM_NETWORK" ] || NPM_NETWORK="mailserver_default"
}

check_sources() {
  local f
  for f in Cargo.toml Cargo.lock Dockerfile src/lib.rs src/main.rs; do
    [ -f "$SCRIPT_DIR/$f" ] || die "нет $SCRIPT_DIR/$f — копируйте на сервер всю папку server/share-signal, а не только скрипт"
  done
  grep -q '"/healthz"' "$SCRIPT_DIR/src/lib.rs" \
    || die "$SCRIPT_DIR/src/lib.rs — старая версия. Возьмите папку из свежего архива."
}

# ── Файлы службы ────────────────────────────────────────────────────────────
copy_code() {
  install -d -m 755 "$APP_DIR"
  install -m 644 "$SCRIPT_DIR/Cargo.toml" "$SCRIPT_DIR/Cargo.lock" "$SCRIPT_DIR/Dockerfile" "$APP_DIR/"
  rm -rf "${APP_DIR:?}/src"
  cp -r "$SCRIPT_DIR/src" "$APP_DIR/src"
  chmod -R u=rwX,go=rX "$APP_DIR/src"
  if [ -f "$SCRIPT_DIR/README.md" ]; then install -m 644 "$SCRIPT_DIR/README.md" "$APP_DIR/README.md"; fi
  printf 'target\n*.md\n*.yml\n' > "$APP_DIR/.dockerignore"; chmod 644 "$APP_DIR/.dockerignore"
  write_compose
  ok "файлы службы: $APP_DIR"
}

write_compose() {
  local nets
  if [ "${DC_V2:-1}" = 1 ]; then
    nets="networks:
  npm:
    external: true
    name: $NPM_NETWORK"
  else
    nets="networks:
  npm:
    external:
      name: $NPM_NETWORK"
  fi
  {
    echo "# Создано install-share-signal.sh $SCRIPT_VERSION. Контейнер в сети Nginx Proxy Manager."
    [ "${DC_V2:-1}" = 1 ] || echo 'version: "2.1"'
    cat <<EOF
services:
  $CONTAINER:
    build: .
    image: $IMAGE
    container_name: $CONTAINER
    environment:
      - LISTEN=0.0.0.0:$PORT
    expose:
      - "$PORT"
    restart: always
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    mem_limit: 64m
    pids_limit: 128
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://127.0.0.1:$PORT/share/api/v1/healthz"]
      interval: 60s
      timeout: 5s
      retries: 3
    logging:
      driver: json-file
      options:
        max-size: "1m"
        max-file: "2"
    networks:
      - npm
$nets
EOF
  } > "$APP_DIR/$COMPOSE_FILE"
  chmod 644 "$APP_DIR/$COMPOSE_FILE"
}

# ── Контейнер ───────────────────────────────────────────────────────────────
up() {
  log "Сборка и запуск контейнера $CONTAINER"
  if [ -n "$ROOT" ]; then ok "(тест) docker compose up пропущен"; return 0; fi
  (cd "$APP_DIR" && "${DC[@]}" -f "$COMPOSE_FILE" up -d --build --force-recreate)
  wait_local_health || { docker logs --tail 30 "$CONTAINER" >&2 || true; die "служба не отвечает внутри контейнера (журнал выше)"; }
  ok "контейнер $CONTAINER запущен"
}

local_get() {  # local_get PATH → ответ службы изнутри контейнера
  docker exec "$CONTAINER" wget -qO- "http://127.0.0.1:$PORT/share/api/v1$1" 2>/dev/null || true
}
wait_local_health() {
  local _
  for _ in $(seq 1 20); do local_get /healthz | grep -q '"ok":true' && return 0; sleep 1; done
  return 1
}

print_npm_steps() {
  echo >&2
  log "Осталось открыть адрес $BASE_PATH снаружи — Nginx Proxy Manager (панель http://IP-сервера:81)"
  cat >&2 <<EOF
  Хост ogneliss.ru → Edit → вкладка Advanced: замените раздел «Ogneliss Share»
  на site/nginx-share.conf из этого архива (в конце — раздел «Служба связи»:
  location ^~ /share/api/ → $CONTAINER:$PORT без журнала запросов). Сначала
  скопируйте прежнее содержимое — чтобы вернуть при ошибке. Save.
  Затем: sudo bash $0 check
EOF
}

# ── Команды ─────────────────────────────────────────────────────────────────
cmd_install() {
  need_root; check_sources
  if [ -z "$ROOT" ]; then have docker || die "Docker не найден"; fi
  if [ -n "$ROOT" ]; then DC_V2=1; else compose_cmd; fi
  resolve_base_url
  local first=1
  if [ -f "$APP_DIR/$COMPOSE_FILE" ]; then first=0; fi
  log "share-signal $SCRIPT_VERSION → $APP_DIR (адрес $BASE_URL)"
  if [ -z "$ROOT" ]; then
    detect_network
    docker network inspect "$NPM_NETWORK" >/dev/null 2>&1 \
      || die "сеть Docker «$NPM_NETWORK» не найдена. Укажите сеть Nginx Proxy Manager: --network ИМЯ (docker network ls)"
  else
    NPM_NETWORK="${NPM_NETWORK:-mailserver_default}"
  fi
  ok "сеть Nginx Proxy Manager: $NPM_NETWORK"
  copy_code
  up
  if [ "$first" = 1 ]; then print_npm_steps; fi
  return 0
}

cmd_remove() {
  need_root
  [ -f "$APP_DIR/$COMPOSE_FILE" ] || die "нет $APP_DIR/$COMPOSE_FILE — служба не установлена"
  compose_cmd
  (cd "$APP_DIR" && "${DC[@]}" -f "$COMPOSE_FILE" down)
  ok "контейнер $CONTAINER удалён; каталог $APP_DIR оставлен"
  warn "уберите и раздел «Служба связи» из Advanced хоста в NPM, иначе $BASE_PATH отвечает 502"
}

cmd_check() {
  need_root
  [ -f "$APP_DIR/$COMPOSE_FILE" ] || die "нет $APP_DIR/$COMPOSE_FILE — служба не установлена (sudo bash $0 install)"
  resolve_base_url
  detect_network
  local fails=0 h code body
  log "Проверка share-signal: $BASE_URL"
  if docker ps --filter "name=^$CONTAINER\$" --format '{{.Status}}' 2>/dev/null | grep -q .; then
    ok "контейнер $CONTAINER: $(docker ps --filter "name=^$CONTAINER\$" --format '{{.Status}}')"
  else
    bad "контейнер $CONTAINER не запущен: cd $APP_DIR && docker compose -f $COMPOSE_FILE up -d"
    return 1
  fi
  h="$(local_get /healthz)"
  if printf '%s' "$h" | grep -q '"ok":true'; then
    ok "служба отвечает; комнат сейчас: $(json_val "$h" rooms)"
  else
    bad "служба не отвечает внутри контейнера"; docker logs --tail 20 "$CONTAINER" >&2 || true
    fails=$((fails + 1))
  fi

  body="$(curl -sS --max-time 10 "$BASE_URL/healthz" 2>/dev/null || true)"
  if printf '%s' "$body" | grep -q '"ok":true'; then
    ok "снаружи отвечает: $BASE_URL/healthz"
  else
    code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "$BASE_URL/healthz" 2>/dev/null || true)"
    bad "снаружи не отвечает: $BASE_URL/healthz (HTTP ${code:-нет связи})"
    case "$code" in
      301|302|404) warn "запрос ушёл в правила сайта — в Advanced хоста $BASE_HOST нет раздела location ^~ /share/api/ (вставьте site/nginx-share.conf)" ;;
      502|503|504) warn "NPM не достучался до $CONTAINER:$PORT — контейнер не в сети NPM ($NPM_NETWORK)? docker network connect $NPM_NETWORK $CONTAINER" ;;
      000|'') warn "нет связи с $BASE_HOST: DNS или сертификат" ;;
    esac
    fails=$((fails + 1))
  fi
  echo >&2
  if [ "$fails" = 0 ]; then log "${C_G}Всё в порядке.${C_0}"
  else log "${C_R}Проблем: $fails${C_0} (подробно — README.md, раздел «Если не работает»)"; return 1; fi
}

main() {
  parse_args "$@"
  case "$CMD" in
    help) usage ;;
    install|update) cmd_install ;;
    check) cmd_check || exit 1 ;;
    remove) resolve_base_url; cmd_remove ;;
  esac
}

main "$@"
