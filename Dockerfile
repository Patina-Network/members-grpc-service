FROM lukemathwalker/cargo-chef:0.1.78-rust-1-alpine3.24 AS chef

WORKDIR /app

RUN apk add --no-cache 'musl-dev=~1.2.6' 'protobuf-dev=~31.1'

FROM chef AS planner

COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
COPY src ./src
COPY .cargo ./.cargo

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build

COPY --from=planner /app/recipe.json ./
COPY .cargo ./.cargo

# Only invalidated when Cargo.toml/Cargo.lock change, so dependencies come from the layer cache.
RUN cargo chef cook --release --locked --recipe-path recipe.json

COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
COPY src ./src
COPY .sqlx ./.sqlx

RUN cargo build --release --locked

FROM mirror.gcr.io/library/alpine:3.24.3 AS server-runtime

RUN addgroup -g 10001 -S grpc-server \
  && adduser -u 10001 -S -G grpc-server -h /app grpc-server

WORKDIR /app

COPY --from=build /app/target/release/members-grpc-service ./

ENV ENVIRONMENT=production

USER 10001:10001

EXPOSE 3000 50051

ENTRYPOINT ["./members-grpc-service"]
