FROM mirror.gcr.io/library/rust:1-alpine3.24 AS build

WORKDIR /app

RUN apk add --no-cache musl-dev=1.2.6-r2 protobuf-dev=31.1-r1

COPY Cargo.toml Cargo.lock build.rs ./
COPY proto ./proto
COPY src ./src

RUN cargo build --release --locked

FROM mirror.gcr.io/library/alpine:3.23.3 AS server-runtime

RUN addgroup -g 10001 -S grpc-server \
  && adduser -u 10001 -S -G grpc-server -h /app grpc-server

WORKDIR /app

COPY --from=build /app/target/release/hello-world-grpc-service ./

ENV ENVIRONMENT=production

USER 10001:10001

EXPOSE 3000 50051

ENTRYPOINT ["./hello-world-grpc-service"]
