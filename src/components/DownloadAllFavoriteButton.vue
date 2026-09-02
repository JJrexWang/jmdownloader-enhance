<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import { useStore } from '../store.ts'
import { commands, events } from '../bindings.ts'
import { NButton, NPopconfirm, useMessage, useNotification, NotificationReactive } from 'naive-ui'

const store = useStore()

const popConfirmShowing = ref<boolean>(false)

const message = useMessage()
const notification = useNotification()

// 单张 overview 卡片聚合整个流程的进度
const overview = ref({
  totalComics: 0,
  // 已经「处理过」(成功结束 or 失败跳过)的本数
  doneCount: 0,
  // 当前正在处理的本(0-based, -1 表示还没进入逐本阶段)
  currentIndex: -1,
  currentComicTitle: '',
  // 当前这本漫画要创建的下载任务总数/已完成数
  currentChaptersTotal: 0,
  currentChaptersDone: 0,
  failedCount: 0,
  failedTitles: [] as string[],
})

let prepareMessage: ReturnType<typeof message.loading> | undefined
let overviewNotification: NotificationReactive | undefined

function renderOverview() {
  const o = overview.value
  const currentDisplay = o.totalComics > 0 ? Math.min(o.currentIndex + 1, o.totalComics) : 0
  const lines: ReturnType<typeof h>[] = []

  if (o.totalComics > 0) {
    lines.push(h('div', `进度: ${o.doneCount}/${o.totalComics}`))
  }

  if (o.currentIndex >= 0 && o.currentComicTitle) {
    const titleText = o.totalComics > 0
      ? `当前(${currentDisplay}/${o.totalComics}): ${o.currentComicTitle}`
      : `当前: ${o.currentComicTitle}`
    lines.push(h('div', titleText))
    if (o.currentChaptersTotal > 0) {
      lines.push(
        h('div', `正在创建下载任务: ${o.currentChaptersDone}/${o.currentChaptersTotal}`),
      )
    }
  }

  if (o.failedCount > 0) {
    lines.push(
      h('div', { style: 'color: #d03050; margin-top: 6px;' }, `失败 ${o.failedCount} 本(详见日志):`),
    )
    const titlesToShow = o.failedTitles.slice(-5)
    for (const t of titlesToShow) {
      lines.push(h('div', { style: 'padding-left: 12px;' }, `· ${t}`))
    }
    if (o.failedTitles.length > 5) {
      lines.push(
        h('div', { style: 'padding-left: 12px; color: #888;' }, `... 还有 ${o.failedTitles.length - 5} 本`),
      )
    }
  }

  return h('div', { style: 'min-width: 280px;' }, lines)
}

function openOverviewNotification() {
  if (overviewNotification !== undefined) return
  overviewNotification = notification.create({
    title: '正在下载整个收藏夹',
    description: () => renderOverview(),
    type: 'info',
    duration: 0,
    closable: true,
  })
}

function closeOverviewNotification() {
  if (overviewNotification !== undefined) {
    overviewNotification.destroy()
    overviewNotification = undefined
  }
}

function resetOverview() {
  overview.value = {
    totalComics: 0,
    doneCount: 0,
    currentIndex: -1,
    currentComicTitle: '',
    currentChaptersTotal: 0,
    currentChaptersDone: 0,
    failedCount: 0,
    failedTitles: [],
  }
}

function cleanupAll() {
  if (prepareMessage !== undefined) {
    prepareMessage.destroy()
    prepareMessage = undefined
  }
  closeOverviewNotification()
  resetOverview()
}

onMounted(async () => {
  await events.downloadAllFavoritesEvent.listen(({ payload }) => {
    if (payload.event === 'GetFavoritesStart') {
      resetOverview()
      prepareMessage = message.loading('正在获取收藏夹', { duration: 0 })
    } else if (payload.event === 'GetComicsProgress') {
      const { current, total, currentComicTitle } = payload.data
      if (overview.value.totalComics === 0) {
        overview.value.totalComics = total
      }
      overview.value.currentIndex = current - 1
      overview.value.currentComicTitle = currentComicTitle
      // 第一次进入「逐本拉取阶段」就把 loading toast 换成 overview 卡片,
      // 这样即便最终所有本都已下完/全部失败,卡片也至少闪一下,
      // 用户能感知到确实跑了一轮而不是「点了按钮啥也没发生」。
      if (prepareMessage !== undefined) {
        prepareMessage.destroy()
        prepareMessage = undefined
        openOverviewNotification()
      } else if (overviewNotification === undefined) {
        // 极端兜底:prepareMessage 已经被前面的失败分支销毁,但 overview 还没开
        openOverviewNotification()
      }
    } else if (payload.event === 'StartCreateDownloadTasks') {
      if (prepareMessage !== undefined) {
        prepareMessage.destroy()
        prepareMessage = undefined
      }
      const { comicTitle, total } = payload.data
      overview.value.currentComicTitle = comicTitle
      overview.value.currentChaptersTotal = total
      overview.value.currentChaptersDone = 0
      openOverviewNotification()
    } else if (payload.event === 'CreatingDownloadTask') {
      const { current } = payload.data
      if (current > overview.value.currentChaptersDone) {
        overview.value.currentChaptersDone = current
      }
    } else if (payload.event === 'EndCreateDownloadTasks') {
      overview.value.doneCount++
      overview.value.currentChaptersTotal = 0
      overview.value.currentChaptersDone = 0
    } else if (payload.event === 'FailedComic') {
      const { comicTitle } = payload.data
      // 第一次失败时就接管「获取阶段」的 loading toast,
      // 避免所有本都失败/都已经下完时,loading 一直转圈不消失
      if (prepareMessage !== undefined) {
        prepareMessage.destroy()
        prepareMessage = undefined
        openOverviewNotification()
      }
      overview.value.failedCount++
      overview.value.failedTitles.push(comicTitle)
      overview.value.doneCount++
      overview.value.currentChaptersTotal = 0
      overview.value.currentChaptersDone = 0
    } else if (payload.event === 'GetComicsEnd') {
      const failed = overview.value.failedCount
      const total = overview.value.totalComics
      const succeeded = Math.max(0, total - failed)
      // 兜底:即便中途没机会销毁,也在这里清掉
      if (prepareMessage !== undefined) {
        prepareMessage.destroy()
        prepareMessage = undefined
      }
      closeOverviewNotification()
      if (failed === 0) {
        message.success(
          total > 0
            ? `收藏夹下载任务创建完成:共处理 ${total} 本,全部成功`
            : '收藏夹中没有需要下载的漫画',
          { duration: 5000 },
        )
      } else {
        const last = overview.value.failedTitles.slice(-5)
        const more = overview.value.failedTitles.length - last.length
        const moreHint = more > 0 ? ` ...还有 ${more} 本` : ''
        const desc = () =>
          h('div', {}, [
            h('div', '收藏夹下载任务创建完成'),
            h('div', `共 ${total} 本,成功 ${succeeded} 本,失败 ${failed} 本(完整列表见日志):`),
            ...last.map((t) => h('div', { style: 'padding-left: 12px;' }, `· ${t}`)),
            moreHint ? h('div', { style: 'padding-left: 12px; color: #888;' }, moreHint) : null,
          ])
        message.warning(desc, { duration: 8000 })
      }
      resetOverview()
    }
  })
})

async function agree() {
  if (store.config === undefined) {
    return
  }

  store.config.imgDownloadIntervalSec = Math.max(1, Math.floor(store.config.imgConcurrency / 5))
  store.config.chapterDownloadIntervalSec = Math.min(10, Math.floor(store.config.imgConcurrency * 3))

  popConfirmShowing.value = false

  const result = await commands.downloadAllFavorites()
  if (result.status === 'error') {
    console.error(result.error)
    cleanupAll()
    return
  }
}

async function reject() {
  popConfirmShowing.value = false
  const result = await commands.downloadAllFavorites()
  if (result.status === 'error') {
    console.error(result.error)
    cleanupAll()
    return
  }
}

function handleDownloadClick() {
  // 直接进入确认对话框,不再有倒计时
}
</script>

<template>
  <n-popconfirm :positive-text="null" :negative-text="null" v-model:show="popConfirmShowing">
    <div class="flex flex-col">
      <div>下载整个收藏夹是个大任务</div>
      <div>为了减轻禁漫服务器压力</div>
      <div>将自动调整配置中的下载间隔</div>
      <div>
        <span>之后你随时可以在右上角的</span>
        <span class="bg-gray-2 px-1">配置</span>
        <span>调整</span>
      </div>
    </div>

    <template #action>
      <n-button size="small" @click="reject">
        <span>不调整直接下载</span>
      </n-button>
      <n-button size="small" type="primary" @click="agree">调整并下载</n-button>
    </template>

    <template #trigger>
      <n-button type="primary" size="small" @click="handleDownloadClick">下载整个收藏夹</n-button>
    </template>
  </n-popconfirm>
</template>
