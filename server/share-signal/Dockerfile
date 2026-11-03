# Служба связи Ogneliss Share — контейнер за Nginx Proxy Manager.
# Собирается установщиком install-share-signal.sh: Alpine (musl) →
# статический бинарник в минимальном образе.

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src

# Сначала только зависимости: этот слой кэшируется, пока не менялись
# Cargo.toml и Cargo.lock, и пересборка после правок кода идёт быстро.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && touch src/lib.rs && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
RUN touch src/lib.rs src/main.rs && cargo build --release --locked

# Итог: Alpine ради wget из busybox (проверка здоровья в compose и в
# установщике: docker exec … wget). Пользователь без прав, uid 10001.
FROM alpine:3.22
RUN adduser -D -H -u 10001 -s /sbin/nologin signal
COPY --from=build /src/target/release/ogneliss-signal /usr/local/bin/ogneliss-signal
USER 10001:10001
ENV LISTEN=0.0.0.0:8789
EXPOSE 8789
ENTRYPOINT ["/usr/local/bin/ogneliss-signal"]
