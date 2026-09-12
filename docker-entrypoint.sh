#!/bin/sh
# =============================================================================
# docker-entrypoint.sh —— 启动期修正挂载卷属主,然后以降权用户跑 server。
# =============================================================================
# 设计目标:
#   容器以 root 跑这个 entrypoint,自动适配两类环境:
#     A) 普通 Docker / 容器内是真 root:
#        - chown 10001:10001 成功 -> 降权到 jm 跑 server (业务进程非 root,安全)
#     B) Rootless Docker / Podman (容器内 root 是 fake root,只能改映射范围内 uid):
#        - chown 10001 失败 -> 直接以 root 跑 server (能写,但日志/cookie 归 root)
#   两种模式都让 server 能跑起来,不会再因 EACCES 在 paths_from_env() 挂掉。
#
#   行为:
#     1. 探测 chown 是否可用:touch 一个文件并 chown 到 10001,失败就 fallback
#     2. 失败模式:把 /config /downloads chmod 777 让所有 uid 都能写
#        (rootless 用户能立刻用,代价是失去了隔离;但 server 本来就要监听 0.0.0.0)
#     3. 成功模式:chown -R 10001:10001 /config /downloads,再 gosu 降权
# =============================================================================

set -eu

JM_UID=10001
JM_GID=10001

echo "[entrypoint] uid=$(id -u) starting; probing chown capability..."

# 探测 chown 是否能成功改到容器内 uid 10001
PROBE=/config/.entrypoint_chown_probe
rm -f "$PROBE" 2>/dev/null || true
if touch "$PROBE" 2>/dev/null && chown "${JM_UID}:${JM_GID}" "$PROBE" 2>/dev/null; then
    rm -f "$PROBE"
    CHOWN_OK=1
    echo "[entrypoint] chown ${JM_UID}:${JM_GID} OK -> 走降权路径"
else
    rm -f "$PROBE" 2>/dev/null || true
    CHOWN_OK=0
    echo "[entrypoint] chown ${JM_UID}:${JM_GID} FAILED -> 走 root 路径 (rootless docker?)"
fi

if [ "$CHOWN_OK" = "1" ]; then
    # 修正挂载卷属主
    for d in /config /downloads; do
        if [ -d "$d" ]; then
            chown -R "${JM_UID}:${JM_GID}" "$d" 2>/dev/null || true
        fi
    done
    exec gosu "${JM_UID}:${JM_GID}" /app/server "$@"
else
    # Rootless 场景:没法 chown,直接把挂载卷 chmod 让所有 uid 都能写
    for d in /config /downloads; do
        if [ -d "$d" ]; then
            chmod -R a+rwX "$d" 2>/dev/null || true
        fi
    done
    # 以当前 uid (root / fake root) 跑 server,能写文件
    exec /app/server "$@"
fi
