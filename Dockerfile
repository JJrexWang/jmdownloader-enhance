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

# 编译期系统依赖：
#   libssl-dev / zlib1g-dev / pkg-config   —— openssl-sys
#   libgtk-3-dev / libsoup-3.0-dev        —— tauri-runtime-wry -> gdk-sys / gtk-sys
#   libwebkit2gtk-4.1-dev                 —— wry -> webkit2gtk-sys
#   librsvg2-dev                          —— tauri icon 处理
#   libayatana-appindicator3-dev          —— libappindicator-sys (libappindicator trait-dep)
# runtime stage 用 debian-slim,不挂这些,镜像体积不变。
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        zlib1g-dev \
        ca-certificates \
        libgtk-3-dev \
        libsoup-3.0-dev \
        libwebkit2gtk-4.1-dev \
        librsvg2-dev \
        libayatana-appindicator3-dev \
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
# gosu: entrypoint 用来从 root 降权到非 root 用户
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        libssl3 \
        zlib1g \
        ca-certificates \
        gosu \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --create-home --uid 10001 jm

WORKDIR /app

# 从 builder 拷出 release 二进制
# cargo 从 /build/src-tauri 跑,manifest 在 ./src-tauri/Cargo.toml,
# 默认 target dir 跟着 manifest 走,所以二进制在 src-tauri 子目录下。
COPY --from=builder /build/src-tauri/src-tauri/target/release/server /app/server

# 数据目录：
#   /config    —— 持久化配置、cookies、日志（推荐挂卷）
#   /downloads —— 下载根目录（推荐挂卷，且在 Config.downloadDir 里改成它）
# 注意:这里不预 chown,挂上去后属主会被宿主卷覆盖,
#       让 entrypoint 在启动期根据实际属主决定是否修复
RUN mkdir -p /config /downloads

# entrypoint 脚本:启动期 chown 挂载卷 + 降权执行 server
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# 注意:不设 USER,让 entrypoint 默认以 root 跑,
#       内部再 gosu 降权到 jm(uid 10001)。
# 如果 compose 里显式 user: "10001:10001" 也兼容,entrypoint 会跳过 chown 直接 exec。

ENV JM_CONFIG_DIR=/config \
    JM_PORT=8080 \
    RUST_LOG=info

EXPOSE 8080

# entrypoint 先 chown 挂载卷 / 降权,再 exec /app/server
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["/app/server"]
