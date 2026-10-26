# Ретранслятор (на развитие)

> Сейчас не планируется: сборки Ogneliss Share используют общедоступные
> ретрансляторы n0, своих серверов у проекта нет. Здесь — что понадобится,
> если когда-нибудь появится свой или арендованный ретранслятор.

## Зачем он нужен

Когда два устройства не могут соединиться напрямую (разные сети, строгий
NAT, мобильный интернет), данные идут через ретранслятор. Он пересылает уже
зашифрованные пакеты: содержимого файлов не видит, видит ключи устройств,
IP-адреса, время и объём. Через него же устройства договариваются о прямом
соединении («пробивка» NAT).

Общедоступные ретрансляторы n0 бесплатны, но ограничивают скорость и не
дают гарантий доступности; авторы iroh рекомендуют их для разработки и
тестирования ([docs.iroh.computer/concepts/relays](https://docs.iroh.computer/concepts/relays)).
Альтернативы — платные ретрансляторы n0 или свой сервер. Свой сервер
означает весь трафик тех передач, что не смогли пойти напрямую, и
ответственность за него.

## Как подключить свой в сборке

Адреса ретрансляторов задаются при сборке переменной `OGNELISS_RELAYS`
(через запятую), код — `Relays::from_build` в `rust/src/node.rs`:

- в GitHub: Settings → Secrets and variables → Actions → **Variables** →
  `OGNELISS_RELAYS` = `https://relay.example.org`;
- локально: `OGNELISS_RELAYS=https://relay.example.org flutter build apk`.

Без переменной — общедоступные ретрансляторы n0. Сборки с разными
ретрансляторами совместимы: адрес ретранслятора собеседника приходит в
QR-коде и в приветствии, и устройство подключается через него.

## Сервер (кратко)

Программа — `iroh-relay` той же версии, что iroh в `rust/Cargo.toml`:

```bash
cargo install iroh-relay --version 1.4.1 --features server --locked
```

Порты: 80/TCP (выпуск сертификата Let's Encrypt), 443/TCP (ретранслятор),
7842/UDP (определение внешнего адреса для пробивки NAT). Сертификат нужен
от общепризнанного центра: приложение проверяет его по встроенному списку
корневых сертификатов Mozilla.

Пример `relay.toml`:

```toml
enable_relay = true
http_bind_addr = "[::]:80"
enable_quic_addr_discovery = true
enable_metrics = false

[tls]
cert_mode = "LetsEncrypt"
hostname = "relay.example.org"
contact = "admin@example.org"
cert_dir = "/var/lib/iroh-relay/certs"

# Защита от перегрузки: новые подключения и скорость на одно устройство.
[limits]
accept_conn_limit = 50.0
accept_conn_burst = 200

[limits.client.rx]
bytes_per_second = 4194304
max_burst_bytes = 16777216
```

Запуск: `iroh-relay --config-path /etc/iroh-relay/relay.toml` (под
отдельным пользователем, через systemd).
