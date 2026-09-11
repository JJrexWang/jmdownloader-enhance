#!/bin/sh
# =============================================================================
# docker-entrypoint.sh —— 启动期修正挂载卷属主,然后以降权用户跑 server。
# =============================================================================
# 设计目标:
#   容器以 root 跑这个 entrypoint,把宿主挂上来的 /config /downloads
#   chown 到容器内非 root 用户(uid/gid = 10001,用户名 jm),
#   再用 gosu 降权执行 /app/server。
#
#   这样:
#     - 宿主机 ./config 首次 clone 出来是 root:root,不用手动 chown
#     - 容器内始终以非 root 用户跑业务进程(更安全)
#     - 任何 host 端用户用 docker compose 拉起来就能直接跑
#
# 兼容性:
#   - 如果 docker-compose.yml 里显式 user: "10001:10001" 让容器以 jm 跑,
#     chown 会因无权限失败,被 || true 吞掉,然后直接 exec /app/server。
#   - 默认以 root 跑(没显式 user),走 chown + gosu 降权路径。
# =============================================================================

set -eu

JM_UID=10001
JM_GID=10001

# 修正挂载卷属主。
#   - 宿主卷是 root:root 或别的 uid 时,这里能改成 jm:jmid;
#   - 当前不是 root(已经被 compose 的 user 强制为 10001)时,chown 会失败,
#     但反正已经是 10001:10001 了,吞掉错误即可。
for d in /config /downloads; do
    if [ -d "$d" ]; then
        chown -R "${JM_UID}:${JM_GID}" "$d" 2>/dev/null || true
    fi
done

# 降权执行
if [ "$(id -u)" = "0" ]; then
    exec gosu "${JM_UID}:${JM_GID}" /app/server "$@"
else
    exec /app/server "$@"
fi
