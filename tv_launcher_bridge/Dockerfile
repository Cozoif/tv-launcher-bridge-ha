FROM rust:1.95-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY ha_integration ./ha_integration
RUN cargo build --locked --release --no-default-features --bin tv-bridge-addon
FROM debian:bookworm-slim
LABEL io.hass.name="TV Launcher Bridge" io.hass.description="PC-free wireless ADB bridge" io.hass.arch="amd64" io.hass.type="addon" io.hass.version="0.3.0"
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/tv-bridge-addon /usr/local/bin/tv-bridge-addon
COPY run.sh ingress.py index.html /opt/bridge/
RUN chmod +x /opt/bridge/run.sh
ENTRYPOINT ["/opt/bridge/run.sh"]
