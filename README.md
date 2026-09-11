<p align="center">
    <img src="https://github.com/user-attachments/assets/9d164ecc-e6ae-4e50-a497-b3ca79344ccf" style="align-self: center"/>
</p>

# 📚 禁漫天堂下载器（个人增强版）

一个用于 18comic.vip 禁漫天堂 jmcomic 18comic 的多线程下载器，带图形界面，带收藏夹，**免费下载收费的漫画**，下载速度飞快。图形界面基于[Tauri](https://v2.tauri.app/start/)

> 本仓库是从 [lanyeeee/jmcomic-downloader](https://github.com/lanyeeee/jmcomic-downloader) fork 的**个人增强版**。  
> 在保留上游功能的基础上,加入了下文「[✨ 相对上游的增强](#-相对上游的增强)」一节里列出的功能。  
> 上游 bugfix 仍会通过 `upstream/main` 同步合并进来。  
> 编译好的 deb 包见 [Release 页面](https://github.com/JJrexWang/jmdownloader-enhance/releases)。

**如果本项目对你有帮助，欢迎点个 Star⭐ 支持！你的支持是我持续更新维护的动力🙏**

# ✨ 相对上游的增强

## 🗜️ 章节归档(zip / cbz)

下载章节时,可以把整个章节(图片 + 章节元数据 json)直接打包成 zip 或 cbz,大幅减少磁盘上散落的文件数量。

- 在 `配置` → `下载` → `章节归档` 处可选 `不打包` / `.zip` / `.cbz`
- 漫画目录下的章节子目录会被打包成单个压缩包,**章节元数据文件也一并打进压缩包**,所以「导出 CBZ / 导出 PDF」等功能依然能识别章节里的图片
- 压缩包里**嵌入了 `chapterId`**,本地库扫描不会因为压缩包就漏识别
- 适用场景:漫画一多、磁盘 inode 紧张、只想用阅读器看 `.cbz` 的用户

## 🔤 中文归一化(简繁转换)

禁漫对同一本漫画在不同登录语言下可能返回简体 / 繁体,导致落地目录被开成两个不同的文件夹。本增强把作者和漫画名在写入磁盘前用 OpenCC 做归一化:

- `配置` → `下载` → `中文归一化` 处可选 `不转换` / `转为简体` / `转为繁体`
- 只对**汉字**做转换,**日文假名 / 韩文 Hangul / 英文 / 数字 / 标点不会被连带改写**
- 漫画内部识别仍然用 `comicId`,所以同一本漫画不会因为目录名归一化而误判为多本

## 🛡️ 缺失图片容忍阈值

上游行为是只要一张图没下到就整章作废。本增强加了一个阈值:

- `配置` → `下载` → `缺失图片容忍`
- 当某章节下载完成后,**缺失图片数 ≤ 阈值** 就视为下载成功(只在日志里告警),超过才作废
- 阈值默认保留上游的严格行为,设为 `0` 即与上游一致
- 失败 / 告警的章节会在日志里汇总(`chapter-download-warning` / `chapter-download-failure`),方便手动补图

## 🔕 关闭错误通知弹窗

下载量大 / 网络偶尔抽风时,ERROR 级弹窗会很烦人。

- `配置` → `下载` → `关闭错误通知弹窗`
- 启用后失败不再以右下角弹窗形式打扰,**实时日志与文件日志仍然会记录**
- 事后从 `日志` 对话框或日志文件里排查

## 🪟 下载收藏夹 / 更新库存的 overview 进度卡片

把「每本漫画弹一张 loading toast」换成「整轮只持续展示一张通知卡」:

- 卡片标题固定 `正在下载整个收藏夹` / `正在更新库存`
- 正文实时刷新:
  - `进度: 已处理/总数`
  - `当前(i/N): <漫画标题>` ← 你能看到「现在在处理哪一本」
  - `正在创建下载任务: x/y`(本内逐章节进度)
  - 失败时显示 `失败 K 本(详见日志)` + 最近 5 个失败标题
- 全部完成时弹一张成功 / 告警总结 toast,把整轮的失败标题展开列出

## ⚡ 性能优化(应对大型本地库存)

上游 `更新库存` 每处理一本漫画就要重建一次本地目录树 id→dir 映射,本地库存一多就慢到怀疑人生。本增强做了:

- `id→dir` 映射**整轮只构建一次**,后台章节下载完成触发的 invalidate 也不会让本轮循环里反复重建
- `进度页` 跨 pane 同步任务**只在目标 pane 已经加载过数据时才发**,避免无效广播
- 多个 `id→dir` 调用命中**同一份缓存**,不再每次都全目录 walk

实测:本地库存 1000+ 本时,「更新库存」从十几秒压缩到秒级;`进度页` 切换不再卡顿。

# 🖥️ 图形界面

![image](https://github.com/user-attachments/assets/2ec6e5f9-a211-4325-8671-0a15f4bcba6c)

# 📖 使用方法

#### 🚀 不使用收藏夹

1. **不需要登录**，直接使用`漫画搜索`，选择要下载的漫画，点击后进入`章节详情`
2. 在`章节详情`勾选要下载的章节，点击`下载勾选章节`按钮开始下载
3. 下载完成后点击`打开下载目录`按钮查看结果

#### ⭐ 使用收藏夹

1. 点击`账号登录`按钮完成登录
2. 使用`漫画收藏`，选择要下载的漫画，点击后进入`章节详情`
3. 在`章节详情`勾选要下载的章节，点击`下载勾选章节`按钮开始下载
4. 下载完成后点击`打开下载目录`按钮查看结果

📹 下面的视频是完整使用流程，**没有H内容，请放心观看**

https://github.com/user-attachments/assets/46096bd9-1fde-4474-b297-0f4389dbe770

# ❓ 常见问题

- [为什么下载过程中CPU占用很高](https://github.com/lanyeeee/jmcomic-downloader/discussions/11)
- [使用Ubuntu22.04时，搜索结果和收藏夹无法加载封面图](https://github.com/lanyeeee/jmcomic-downloader/discussions/31)
- [使用Ubuntu24.04时，窗口全白](https://github.com/lanyeeee/jmcomic-downloader/discussions/32)

# 📚 哔咔漫画下载器

[![picacomic-downloader](https://github-readme-stats-fast.vercel.app/api/pin/?username=lanyeeee&repo=picacomic-downloader)](https://github.com/lanyeeee/picacomic-downloader)

# ⚠️ 关于被杀毒软件误判为病毒

对于个人开发者来说，这个问题几乎是无解的(~~需要购买数字证书给软件签名，甚至给杀毒软件交保护费~~)  
我能想到的解决办法只有：

1. 根据下面的**如何构建(build)**，自行编译
2. 希望你相信我的承诺，我承诺你在[Release页面](https://github.com/lanyeeee/jmcomic-downloader/releases)下载到的所有东西都是安全的。切勿轻信他人分享的文件，请**仅**在[Release页面](https://github.com/lanyeeee/jmcomic-downloader/releases)下载。任何不是从该页面下载的版本均可能**已被篡改**并**真的包含病毒**(而非误报)，包括但不限于`网盘`、`通过邮箱或社交软件分享`、`issue或discussion里的文件`、`其他fork(仓库)`、`其他网站`

# 🐳 Docker 部署（HTTP server 模式）

除了桌面端，本仓库已经把核心业务抽成 `Arc<dyn AppContext>`，因此同一份代码也能跑成
**纯 HTTP 服务**，打成 Docker 镜像部署在 NAS / 服务器上，配合外部前端或脚本用。

镜像只构建 `src/bin/server.rs`（不带 Tauri 运行时），约 60MB，启动后监听 `0.0.0.0:${JM_PORT:-8080}`，
提供 axum REST + SSE（业务事件推送）端点。

## 快速开始

```bash
# 1. 构建并后台启动
docker compose up -d --build

# 2. 验证
curl http://localhost:8080/health
# {"service":"jmcomic-downloader","status":"ok"}

# 3. 看日志
docker compose logs -f
ls -lh config/logs/   # 文件日志也持久化在 config 卷里
```

## 关键端点

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/health` | 健康检查 |
| GET | `/events` | SSE 业务事件流（下载进度 / 完成 / 失败） |
| GET / POST | `/config` | 读 / 写 `Config`（含代理、下载目录、并发等） |
| POST | `/login` | `{ "username", "password" }`，返回 user profile |
| GET | `/user-profile` | 当前登录用户信息 |
| POST | `/search` | `{ "keyword", "page", "sort" }` |
| GET | `/comic/:id` | 单本漫画元信息 + 章节列表 |
| POST | `/favorites` | `{ "folder_id", "page", "sort" }` |
| GET | `/weekly-info` | 本周必看分类 |
| POST | `/weekly` | `{ "category_id", "type_id" }` |
| POST | `/download/task` / `/tasks` | 单章节 / 多章节下载 |
| POST | `/download/pause` / `/resume` / `/delete` | 任务控制 |
| POST | `/download/comic` | `{ "aid" }` 整本漫画入队 |
| POST | `/download/all-favorites` | 一键下载所有收藏夹 |
| POST | `/download/update-downloaded` | 重建本地已下载索引 |
| POST | `/export/cbz` / `/pdf` / `/cbz/chapters` / `/pdf/chapters` | 导出整本或指定章节 |
| GET | `/logs/size` | 文件日志大小 |

详细 endpoint 实现见 `src-tauri/src/bin/server.rs`。

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `JM_PORT` | `8080` | 容器内监听端口（宿主机侧用 `docker-compose.yml` 里的 `ports`） |
| `JM_CONFIG_DIR` | `/config` | 配置 / cookies / 日志根目录（持久化卷） |
| `JM_DOWNLOADS_DIR` | `/downloads` | 仅作为建议；实际生效值是 `/config/config.json` 里的 `downloadDir` |
| `JM_USERNAME` / `JM_PASSWORD` | 空 | 设置后启动时自动 `POST /login` |
| `RUST_LOG` | `info` | 标准 tracing 语法：`debug,jmcomic_downloader_lib=trace` |

## 卷 / 数据布局

```
./config/         <- 挂到容器 /config
  ├── config.json    # 运行期配置（端口、代理、并发、downloadDir 等）
  ├── cookies.json   # JM 登录态
  └── logs/          # 滚动日志（按天分割）
./downloads/      <- 挂到容器 /downloads
                     # 漫画实际落盘目录
                     # 想改路径，编辑 config.json 里的 downloadDir 即可
```

容器以 uid `10001` 运行；首次启动会自动建好目录。如果宿主 `./config` 的属主不是 10001，
可用 `chown -R 10001:10001 ./config ./downloads`，或者在 `docker-compose.yml` 里把 `user:` 改成 `"0:0"`（不推荐，但能用）。

## 自定义构建 / 单镜像

```bash
# 单镜像构建（不用 compose）
docker build -t jmcomic-downloader:local .

# 直接跑
docker run -d --name jm-server   -p 8080:8080   -v $(pwd)/config:/config   -v $(pwd)/downloads:/downloads   -e RUST_LOG=info   jmcomic-downloader:local

# 进入容器调试
docker exec -it jm-server /bin/bash
```

## 调试示例：登录 + 搜本子 + 整本下载

```bash
# 1) 登录（可选，启动时设了 JM_USERNAME/JM_PASSWORD 会自动跑）
curl -s -X POST http://localhost:8080/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"你的账号","password":"你的密码"}'

# 2) 搜索
curl -s -X POST http://localhost:8080/search \
  -H 'Content-Type: application/json' \
  -d '{"keyword":"原神","page":1,"sort":"Latest"}' | jq '.SearchResult.content[0:3]'

# 3) 整本下载（拿本子 aid）
curl -s -X POST http://localhost:8080/download/comic \
  -H 'Content-Type: application/json' \
  -d '{"aid":422866}'

# 4) 订阅进度事件流
curl -N http://localhost:8080/events
```

# 🛠️ 如何构建(build)

构建非常简单，一共就3条命令  
~~前提是你已经安装了Rust、Node、pnpm~~

#### 📋 前提

- [Rust](https://www.rust-lang.org/tools/install)
- [Node](https://nodejs.org/en)
- [pnpm](https://pnpm.io/installation)

#### 📝 步骤

#### 1. 克隆本仓库

```
git clone https://github.com/JJrexWang/jmdownloader-enhance.git
```

#### 2.安装依赖

```
cd jmdownloader-enhance
pnpm install
```

#### 3.构建(build)

```
pnpm tauri build
```

# 🤝 提交PR

**PR请基于`develop`分支开发，并提交至`develop`分支**

**如果想新加一个功能，请先开个`issue`或`discussion`讨论一下，避免无效工作**

其他情况的PR欢迎直接提交，比如：

1. 🔧 对原有功能的改进
2. 🐛 修复BUG
3. ⚡ 使用更轻量的库实现原有功能
4. 📝 修订文档
5. ⬆️ 升级、更新依赖的PR也会被接受

# ⚠️ 免责声明

- 本工具仅作学习、研究、交流使用，使用本工具的用户应自行承担风险
- 作者不对使用本工具导致的任何损失、法律纠纷或其他后果负责
- 作者不对用户使用本工具的行为负责，包括但不限于用户违反法律或任何第三方权益的行为

# 💬 其他

任何使用中遇到的问题、任何希望添加的功能，都欢迎提交issue或开discussion交流，我会尽力解决
