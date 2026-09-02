<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { commands, events } from '../../bindings.ts'
import { open } from '@tauri-apps/plugin-dialog'
import { PhFolderOpen, PhGearSix } from '@phosphor-icons/vue'
import { useStore } from '../../store.ts'
import SettingsDialog from '../../dialogs/SettingsDialog/SettingsDialog.vue'
import UncompletedProgresses from './components/UncompletedProgresses.vue'
import CompletedProgresses from './components/CompletedProgresses.vue'
import { ProgressData } from '../../types.ts'
import ExportProgresses from './components/ExportProgresses.vue'
import { NButton, NIcon, NInput, NInputGroup, NInputGroupLabel, NTabPane, NTabs } from 'naive-ui'

const store = useStore()

const settingsDialogShowing = ref<boolean>(false)

const downloadSpeed = ref<string>('')

let unListenDownloadEvent: (() => void) | undefined
onMounted(async () => {
  await events.downloadEvent
    .listen(async ({ payload: { event, data } }) => {
      if (event === 'Speed') {
        downloadSpeed.value = data.speed
      } else if (event === 'Sleeping') {
        const progressData = store.progresses.get(data.chapterId)
        if (progressData !== undefined) {
          progressData.indicator = `将在${data.remainingSec}秒后继续下载`
        }
      } else if (event === 'TaskCreate') {
        const { chapterInfo, downloadedImgCount, totalImgCount } = data

        store.progresses.set(chapterInfo.chapterId, {
          ...data,
          percentage: 0,
          indicator: `排队中 ${downloadedImgCount}/${totalImgCount}`,
        })
      } else if (event === 'TaskUpdate') {
        const { chapterId, state, downloadedImgCount, totalImgCount } = data

        const progressData = store.progresses.get(chapterId)
        if (progressData === undefined) {
          return
        }

        progressData.state = state
        progressData.downloadedImgCount = downloadedImgCount
        progressData.totalImgCount = totalImgCount

        if (state === 'Completed') {
          progressData.chapterInfo.isDownloaded = true
          // 只在相关 tab 实际加载了数据时才同步；否则跳过 4 次 IPC，避免每次章节完成都触发全目录 walk
          if (store.pickedComic !== undefined && store.pickedComic.id === progressData.comic.id) {
            await syncPickedComic()
          }
          if (store.searchResult !== undefined) {
            await syncComicInSearch(progressData)
          }
          if (store.getFavoriteResult !== undefined) {
            await syncComicInFavorite(progressData)
          }
          if (store.getWeeklyResult !== undefined) {
            await syncComicInWeekly(progressData)
          }
        }

        progressData.percentage = (downloadedImgCount / totalImgCount) * 100

        let indicator = ''
        if (state === 'Pending') {
          indicator = `排队中`
        } else if (state === 'Downloading') {
          indicator = `下载中`
        } else if (state === 'Paused') {
          indicator = `已暂停`
        } else if (state === 'Completed') {
          indicator = `下载完成`
        } else if (state === 'Failed') {
          indicator = `下载失败`
        }
        if (totalImgCount !== 0) {
          indicator += ` ${downloadedImgCount}/${totalImgCount}`
        }

        progressData.indicator = indicator
      } else if (event === 'TaskDelete') {
        store.progresses.delete(data.chapterId)
      }
    })
    .then((unListenFn) => {
      unListenDownloadEvent = unListenFn
    })
})

onUnmounted(() => {
  unListenDownloadEvent?.()
})

async function syncPickedComic() {
  if (store.pickedComic === undefined) {
    return
  }
  const result = await commands.getSyncedComic(store.pickedComic)
  if (result.status === 'error') {
    console.error(result.error)
    return
  }
  Object.assign(store.pickedComic, result.data)
}

async function syncComicInSearch(progressData: ProgressData) {
  if (store.searchResult === undefined) {
    return
  }
  const comic = store.searchResult.content.find((comic) => comic.id === progressData.comic.id)
  if (comic === undefined) {
    return
  }
  const result = await commands.getSyncedComicInSearch(comic)
  if (result.status === 'error') {
    console.error(result.error)
    return
  }
  Object.assign(comic, result.data)
}

async function syncComicInFavorite(progressData: ProgressData) {
  if (store.getFavoriteResult === undefined) {
    return
  }
  const comic = store.getFavoriteResult.list.find((comic) => comic.id === progressData.comic.id)
  if (comic === undefined) {
    return
  }
  const result = await commands.getSyncedComicInFavorite(comic)
  if (result.status === 'error') {
    console.error(result.error)
    return
  }
  Object.assign(comic, result.data)
}

async function syncComicInWeekly(progressData: ProgressData) {
  if (store.getWeeklyResult === undefined) {
    return
  }
  const comic = store.getWeeklyResult.list.find((comic) => comic.id === progressData.comic.id)
  if (comic === undefined) {
    return
  }
  const result = await commands.getSyncedComicInWeekly(comic)
  if (result.status === 'error') {
    console.error(result.error)
    return
  }
  Object.assign(comic, result.data)
}

async function showDownloadDirInFileManager() {
  if (store.config === undefined) {
    return
  }
  const result = await commands.showPathInFileManager(store.config.downloadDir)
  if (result.status === 'error') {
    console.error(result.error)
  }
}

async function selectDownloadDir() {
  if (store.config === undefined) {
    return
  }

  const selectedDirPath = await open({ directory: true })
  if (selectedDirPath === null) {
    return
  }

  store.config.downloadDir = selectedDirPath
}
</script>

<template>
  <div v-if="store.config !== undefined" class="flex flex-col flex-1 overflow-auto">
    <div class="flex gap-1 box-border px-2 pt-2.5">
      <n-input-group class="">
        <n-input-group-label size="small">下载目录</n-input-group-label>
        <n-input v-model:value="store.config.downloadDir" size="small" readonly @click="selectDownloadDir" />
        <n-button class="w-10" size="small" @click="showDownloadDirInFileManager">
          <template #icon>
            <n-icon size="20">
              <PhFolderOpen />
            </n-icon>
          </template>
        </n-button>
      </n-input-group>
      <n-button size="small" @click="settingsDialogShowing = true">
        <template #icon>
          <n-icon size="20">
            <PhGearSix />
          </n-icon>
        </template>
        配置
      </n-button>
    </div>
    <n-tabs class="h-full overflow-auto" v-model:value="store.progressesPaneTabName" type="line" size="small">
      <n-tab-pane class="h-full p-0! overflow-auto" name="uncompleted" tab="未完成">
        <UncompletedProgresses />
      </n-tab-pane>
      <n-tab-pane class="h-full p-0! overflow-auto" name="completed" tab="已完成">
        <CompletedProgresses />
      </n-tab-pane>
      <n-tab-pane class="h-full p-0! overflow-auto" name="export" tab="导出进度" display-directive="show">
        <ExportProgresses />
      </n-tab-pane>

      <template #suffix>
        <span class="whitespace-nowrap text-ellipsis overflow-hidden">{{ downloadSpeed }}</span>
      </template>
    </n-tabs>
    <SettingsDialog v-model:showing="settingsDialogShowing" />
  </div>
</template>
