# syntax=docker/dockerfile:1.7
# ----------------------------------------------------------------------------
# JMComic Downloader — HTTP server binary (Docker 部署镜像)
# ----------------------------------------------------------------------------
# 多阶段构建：
#   1. builder : rust:1.83-bookworm 编译 release 版 `server` 二进制
#   2. runtime : debian:bookworm-slim 携带运行时动态库
# 只构建 src/bin/server.rs（不打包 tauri 桌面运行时），镜像约 60MB。
# ----------------------------------------------------------------------------

FROM rust:1.88-bookworm AS builder

# 编译期系统依赖：openssl-sys / pkg-config / zlib / ca-certificates
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        zlib1g-dev \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# 先只复制 Cargo 文件，最大化层缓存命中
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/
RUN mkdir -p src-tauri/src/bin src-tauri/src \
 && echo "fn main() {}" > src-tauri/src/bin/server.rs \
 && echo "" > src-tauri/src/lib.rs

# 让 cargo 解析一次依赖（空二进制也能解析），缓存 registry 与 crates
WORKDIR /build/src-tauri
RUN cargo fetch

# 真实源码到位再编译
COPY src-tauri/ ./src-tauri/

# release 构建，strip + lto 已在 Cargo.toml [profile.release] 启用
RUN cargo build --release --bin server --manifest-path src-tauri/Cargo.toml

# ----------------------------------------------------------------------------

FROM debian:bookworm-slim AS runtime

# 运行时依赖：libssl3（openssl）+ zlib + ca-certificates（HTTPS 调 JM API）
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        libssl3 \
        zlib1g \
        ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --create-home --uid 10001 jm

WORKDIR /app

# 从 builder 拷出 release 二进制
COPY --from=builder /build/src-tauri/target/release/server /app/server

# 数据目录：
#   /config    —— 持久化配置、cookies、日志（推荐挂卷）
#   /downloads —— 下载根目录（推荐挂卷，且在 Config.downloadDir 里改成它）
RUN mkdir -p /config /downloads \
 && chown -R jm:jm /config /downloads

USER jm

ENV JM_CONFIG_DIR=/config \
    JM_PORT=8080 \
    RUST_LOG=info

EXPOSE 8080

# 启动期会创建默认 config.json；
# 首次启动后可在 /config/config.json 里改端口、下载目录、代理等
ENTRYPOINT ["/app/server"]
