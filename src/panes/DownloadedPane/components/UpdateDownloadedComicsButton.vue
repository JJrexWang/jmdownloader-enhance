<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import { NButton, NPopconfirm, useMessage, useNotification, NotificationReactive } from 'naive-ui'
import { commands, events } from '../../../bindings.ts'
import { useStore } from '../../../store.ts'

const message = useMessage()
const notification = useNotification()

const store = useStore()

const popConfirmShowing = ref<boolean>(false)

// 不再有倒计时,按钮始终可点
const rejectButtonDisabled = ref<boolean>(false)

// 单张 overview 卡片聚合整个流程的进度
const overview = ref({
  totalComics: 0,
  doneCount: 0,
  currentIndex: -1,
  currentComicTitle: '',
  currentChaptersTotal: 0,
  currentChaptersDone: 0,
  failedCount: 0,
  failedTitles: [] as string[],
})

let updateMessage: ReturnType<typeof message.loading> | undefined
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
    title: '正在更新库存',
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
  if (updateMessage !== undefined) {
    updateMessage.destroy()
    updateMessage = undefined
  }
  closeOverviewNotification()
  resetOverview()
}

onMounted(async () => {
  await events.updateDownloadedComicsEvent.listen(async ({ payload: updateEvent }) => {
    if (updateEvent.event === 'GetComicStart') {
      resetOverview()
      updateMessage = message.loading('正在获取已下载漫画的最新数据', { duration: 0 })
    } else if (updateEvent.event === 'GetComicProgress' && updateMessage !== undefined) {
      const { current, total, currentComicTitle } = updateEvent.data
      updateMessage.content = `正在获取已下载漫画的最新数据(${current}/${total})`
      if (overview.value.totalComics === 0) {
        overview.value.totalComics = total
      }
      overview.value.currentIndex = current - 1
      overview.value.currentComicTitle = currentComicTitle
    } else if (updateEvent.event === 'CreateDownloadTasksStart') {
      if (updateMessage !== undefined) {
        updateMessage.destroy()
        updateMessage = undefined
      }
      const { comicTitle, total } = updateEvent.data
      overview.value.currentComicTitle = comicTitle
      overview.value.currentChaptersTotal = total
      overview.value.currentChaptersDone = 0
      openOverviewNotification()
    } else if (updateEvent.event === 'CreateDownloadTaskProgress') {
      const { current } = updateEvent.data
      if (current > overview.value.currentChaptersDone) {
        overview.value.currentChaptersDone = current
      }
    } else if (updateEvent.event === 'CreateDownloadTasksEnd') {
      overview.value.doneCount++
      overview.value.currentChaptersTotal = 0
      overview.value.currentChaptersDone = 0
    } else if (updateEvent.event === 'FailedComic') {
      const { comicTitle } = updateEvent.data
      overview.value.failedCount++
      overview.value.failedTitles.push(comicTitle)
      overview.value.doneCount++
      overview.value.currentChaptersTotal = 0
      overview.value.currentChaptersDone = 0
    } else if (updateEvent.event === 'GetComicEnd') {
      const failed = overview.value.failedCount
      const total = overview.value.totalComics
      const succeeded = Math.max(0, total - failed)
      closeOverviewNotification()
      if (failed === 0) {
        message.success(
          total > 0
            ? `库存更新完成:共检查 ${total} 本,全部成功`
            : '本地库存没有需要更新的漫画',
          { duration: 5000 },
        )
      } else {
        message.warning(
          `库存更新完成:共 ${total} 本,成功 ${succeeded} 本,失败 ${failed} 本(详见日志)`,
          { duration: 8000 },
        )
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

  const result = await commands.updateDownloadedComics()
  if (result.status === 'error') {
    console.error(result.error)
    cleanupAll()
    return
  }
}

async function reject() {
  popConfirmShowing.value = false
  const result = await commands.updateDownloadedComics()
  if (result.status === 'error') {
    console.error(result.error)
    cleanupAll()
    return
  }
}

function handleButtonClick() {
  // 不再启动倒计时,按钮始终可点
}
</script>

<template>
  <n-popconfirm :positive-text="null" :negative-text="null" v-model:show="popConfirmShowing">
    <div class="flex flex-col">
      <div>更新库存是个大任务</div>
      <div>为了减轻禁漫服务器压力</div>
      <div>将自动调整配置中的下载间隔</div>
      <div>
        <span>之后你随时可以在右上角的</span>
        <span class="bg-gray-2 px-1">配置</span>
        <span>调整</span>
      </div>
    </div>

    <template #action>
      <n-button size="small" :disabled="rejectButtonDisabled" @click="reject">
        <span>不调整直接下载</span>
      </n-button>
      <n-button size="small" type="primary" @click="agree">调整并下载</n-button>
    </template>

    <template #trigger>
      <n-button size="small" @click="handleButtonClick">更新库存</n-button>
    </template>
  </n-popconfirm>
</template>
