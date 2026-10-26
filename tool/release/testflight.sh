#!/usr/bin/env bash
# Сборка Ogneliss Share для App Store Connect и загрузка в TestFlight.
#
#   tool/release/testflight.sh ios      # iPhone и iPad
#   tool/release/testflight.sh macos    # Mac
#   tool/release/testflight.sh all      # обе платформы подряд
#
# Нужно: Mac с Xcode 26 или новее (App Store Connect с 28.04.2026 принимает
# только такие сборки), Flutter, Rust (rustup), Python 3, CocoaPods.
# Перед первой загрузкой — запись приложения в App Store Connect с
# идентификатором com.ogneliss.share (docs/RELEASE.md, раздел TestFlight).
#
# Переменные окружения:
#   APPLE_TEAM_ID   идентификатор команды Apple, 10 знаков (developer.apple.com →
#                   Membership details → Team ID). Обязательно.
#   ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH
#                   ключ App Store Connect API (файл AuthKey_XXXX.p8): подпись и
#                   загрузка без входа в Xcode. Без них Xcode берёт учётную
#                   запись из Xcode → Settings → Accounts.
#   BUILD_NUMBER    номер сборки; по умолчанию — минуты с 01.01.2026 UTC (растёт сам).
#   UPLOAD=0        только собрать: .ipa / .pkg в dist/, без загрузки.
#
# Пример:
#   export APPLE_TEAM_ID=ABCDE12345
#   export ASC_KEY_ID=XXXXXXXXXX ASC_ISSUER_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
#   export ASC_KEY_PATH=~/keys/AuthKey_XXXXXXXXXX.p8
#   tool/release/testflight.sh all
set -euo pipefail

cd "$(dirname "$0")/../.."
ROOT=$(pwd)

log() { printf '[testflight] %s\n' "$*"; }
die() { printf '[testflight] ошибка: %s\n' "$*" >&2; exit 1; }

TARGET=${1:-}
case "$TARGET" in
  ios|macos|all) ;;
  *) sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac

[[ "$(uname)" == Darwin ]] || die "сборка для App Store — только на Mac"
command -v flutter >/dev/null || die "не найден flutter в PATH"
command -v xcodebuild >/dev/null || die "не найден Xcode (xcodebuild)"
[[ -n "${APPLE_TEAM_ID:-}" ]] || die "укажите APPLE_TEAM_ID — идентификатор команды Apple (10 знаков)"

xcode_major=$(xcodebuild -version | awk 'NR == 1 { split($2, v, "."); print v[1] }')
if (( xcode_major < 26 )); then
  log "⚠ Xcode $xcode_major: App Store Connect принимает только сборки из Xcode 26 или новее"
fi

VERSION=$(sed -n 's/^version: *\([0-9][0-9.]*\).*/\1/p' pubspec.yaml)
[[ -n "$VERSION" ]] || die "в pubspec.yaml нет version"
BUILD_NUMBER=${BUILD_NUMBER:-$(python3 -c 'import datetime as d; t = d.datetime.now(d.timezone.utc) - d.datetime(2026, 1, 1, tzinfo=d.timezone.utc); print(int(t.total_seconds() // 60))')}
UPLOAD=${UPLOAD:-1}

# Ключ App Store Connect API — для подписи (сертификаты и профили Xcode
# создаёт сам) и для загрузки.
AUTH=()
if [[ -n "${ASC_KEY_ID:-}" ]]; then
  [[ -n "${ASC_ISSUER_ID:-}" ]] || die "задан ASC_KEY_ID, но нет ASC_ISSUER_ID"
  [[ -f "${ASC_KEY_PATH:-}" ]] || die "нет файла ключа ASC_KEY_PATH=${ASC_KEY_PATH:-}"
  ASC_KEY_PATH="$(cd "$(dirname "$ASC_KEY_PATH")" && pwd)/$(basename "$ASC_KEY_PATH")"
  AUTH=(-authenticationKeyPath "$ASC_KEY_PATH" -authenticationKeyID "$ASC_KEY_ID"
        -authenticationKeyIssuerID "$ASC_ISSUER_ID")
  log "вход: ключ App Store Connect API $ASC_KEY_ID"
else
  log "вход: учётная запись из Xcode → Settings → Accounts"
fi

if [[ ! -d ios || ! -d macos ]]; then
  python3 tool/bootstrap.py
fi

WORK=build/release
mkdir -p dist "$WORK"

export_options() {  # $1 — upload | export
  cat > "$WORK/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>$1</string>
  <key>teamID</key><string>$APPLE_TEAM_ID</string>
  <key>signingStyle</key><string>automatic</string>
  <key>uploadSymbols</key><true/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict>
</plist>
EOF
}

release() {  # $1 — ios | macos
  local p=$1 destination archive export_dir
  if [[ $p == ios ]]; then destination='generic/platform=iOS'; else destination='generic/platform=macOS'; fi
  archive="$WORK/$p/Runner.xcarchive"
  export_dir="$WORK/$p/export"
  rm -rf "$archive" "$export_dir"

  log "$p: Ogneliss Share $VERSION ($BUILD_NUMBER) — настройка проекта"
  if [[ $p == ios ]]; then
    flutter build ios --config-only --no-codesign --release \
      --build-name="$VERSION" --build-number="$BUILD_NUMBER"
  else
    flutter build macos --config-only --release \
      --build-name="$VERSION" --build-number="$BUILD_NUMBER"
  fi

  log "$p: архив (Xcode подпишет сам: команда $APPLE_TEAM_ID)"
  xcodebuild -workspace "$p/Runner.xcworkspace" -scheme Runner -configuration Release \
    -destination "$destination" -archivePath "$archive" \
    -allowProvisioningUpdates ${AUTH[@]+"${AUTH[@]}"} \
    DEVELOPMENT_TEAM="$APPLE_TEAM_ID" \
    archive

  if [[ $UPLOAD == 1 ]]; then
    log "$p: подпись для App Store и загрузка в App Store Connect"
    export_options upload
  else
    log "$p: подпись для App Store (без загрузки)"
    export_options export
  fi
  xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export_dir" \
    -exportOptionsPlist "$WORK/ExportOptions.plist" \
    -allowProvisioningUpdates ${AUTH[@]+"${AUTH[@]}"}

  if [[ $UPLOAD == 1 ]]; then
    log "$p: загружено. Через 5–30 минут сборка $VERSION ($BUILD_NUMBER) появится в App Store Connect → TestFlight"
  else
    local f
    for f in "$export_dir"/*.ipa "$export_dir"/*.pkg; do
      [[ -e "$f" ]] || continue
      local ext=${f##*.}
      cp "$f" "dist/ogneliss-share-$VERSION-$p.$ext"
      log "$p: dist/ogneliss-share-$VERSION-$p.$ext"
    done
  fi
}

case "$TARGET" in
  ios) release ios ;;
  macos) release macos ;;
  all) release ios; release macos ;;
esac
log "готово"
