# Служба связи Ogneliss Share: сборка в Alpine (musl) → статический бинарник
# в минимальном образе без оболочки. Сборка: docker build -t ogneliss-signal .
# (из каталога server/signal).

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

# Итоговый образ: только бинарник. Пользователь nonroot (uid 65532), оболочки
# нет, поэтому и HEALTHCHECK в образе не задан: проверять /health снаружи.
FROM gcr.io/distroless/static:nonroot
COPY --from=build /src/target/release/ogneliss-signal /ogneliss-signal
USER nonroot:nonroot
EXPOSE 8080
ENTRYPOINT ["/ogneliss-signal"]
