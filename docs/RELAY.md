# Свой ретранслятор на ogneliss.ru

Когда два устройства не могут соединиться напрямую (разные сети, строгий
NAT, мобильный интернет), данные идут через ретранслятор. Он пересылает
уже зашифрованные пакеты: содержимого файлов не видит, видит только ключи
устройств, IP-адреса, время и объём.

Пока в сборке не указан свой ретранслятор, приложение пользуется
общедоступными ретрансляторами n0 (авторы iroh). Для выпуска лучше свой:
предсказуемая доступность из России, никаких сторонних служб, свои
ограничения скорости. Сервер нужен небольшой — 1 ядро, 512 МБ памяти;
расход — только трафик тех передач, что не смогли пойти напрямую.

## 1. DNS

Запись `A` (и `AAAA`, если есть IPv6): `relay.ogneliss.ru` → адрес сервера.

## 2. Порты

| Порт | Протокол | Зачем |
|---|---|---|
| 80 | TCP | выпуск сертификата Let's Encrypt (ACME) |
| 443 | TCP | ретранслятор (HTTPS/WebSocket) |
| 7842 | UDP | определение внешнего адреса для пробивки NAT (QUIC) |

## 3. Установка

На сервере с Rust (`curl https://sh.rustup.rs -sSf | sh`):

```bash
cargo install iroh-relay --version 1.4.1 --features server --locked
sudo install -m 755 ~/.cargo/bin/iroh-relay /usr/local/bin/iroh-relay
sudo useradd --system --home /var/lib/iroh-relay --create-home iroh-relay
sudo mkdir -p /etc/iroh-relay
```

`/etc/iroh-relay/relay.toml`:

```toml
enable_relay = true
http_bind_addr = "[::]:80"
enable_quic_addr_discovery = true
enable_metrics = false

[tls]
cert_mode = "LetsEncrypt"
hostname = "relay.ogneliss.ru"
contact = "oedante@gmail.com"
cert_dir = "/var/lib/iroh-relay/certs"
prod_tls = true

# Защита от перегрузки: не больше 50 новых подключений в секунду
# и 4 МБ/с на одно устройство (с запасом на рывки).
[limits]
accept_conn_limit = 50.0
accept_conn_burst = 200

[limits.client.rx]
bytes_per_second = 4194304
max_burst_bytes = 16777216
```

Сертификат должен быть от общепризнанного центра (Let's Encrypt): приложение
проверяет его по встроенному списку корневых сертификатов Mozilla.

`/etc/systemd/system/iroh-relay.service`:

```ini
[Unit]
Description=iroh relay for Ogneliss Share
After=network-online.target
Wants=network-online.target

[Service]
User=iroh-relay
WorkingDirectory=/var/lib/iroh-relay
ExecStart=/usr/local/bin/iroh-relay --config-path /etc/iroh-relay/relay.toml
AmbientCapabilities=CAP_NET_BIND_SERVICE
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/iroh-relay

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now iroh-relay
journalctl -u iroh-relay -f        # сертификат выпускается при первом запросе
curl -I https://relay.ogneliss.ru  # должен ответить
```

## 4. Сборка приложения со своим ретранслятором

Адреса ретрансляторов задаются при сборке переменной `OGNELISS_RELAYS`
(через запятую):

- в GitHub: Settings → Secrets and variables → Actions → **Variables** →
  `OGNELISS_RELAYS` = `https://relay.ogneliss.ru` — следующие сборки CI
  будут с ним;
- локально: `OGNELISS_RELAYS=https://relay.ogneliss.ru flutter build apk`.

В «Настройки → О приложении → Ретранслятор» видно, какой адрес в сборке.
Со своим ретранслятором сторонние службы не используются совсем: устройства
находят друг друга по последнему известному адресу, ретранслятору и mDNS
в локальной сети.

Сборки с разными ретрансляторами совместимы: адрес ретранслятора
собеседника приходит в QR-коде и в приветствии, и устройство подключается
через него.
