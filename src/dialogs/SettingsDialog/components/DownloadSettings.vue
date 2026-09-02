<script setup lang="ts">
import { ref, watch } from 'vue'
import { useStore } from '../../../store.ts'
import { NCheckbox, NInput, NInputNumber, NRadio, NRadioGroup, NTooltip, useMessage } from 'naive-ui'

const store = useStore()

const message = useMessage()

const dirFmt = ref<string>(store.config?.dirFmt ?? '')
const missingImageThreshold = ref<number>(store.config?.missingImageThreshold ?? 5)

watch([() => store.config?.apiDomainMode, () => store.config?.customApiDomain], () => {
  message.warning('切换线路后可能需要重新登录')
})
</script>

<template>
  <div v-if="store.config !== undefined" class="flex flex-col">
    <span class="font-bold mt-2">下载格式</span>
    <n-radio-group v-model:value="store.config.downloadFormat">
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="Jpeg">jpg</n-radio>
        </template>
        1. 有损
        <span class="text-red">(肉眼看不出)</span>
        <br />
        2. 文件体积小
        <br />
        4. 宽高的上限为65534
        <span class="text-red">(某些条漫可能会超过这个上限导致报错)</span>
        <br />
        3. 编码速度最快
        <br />
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="Png">png</n-radio>
        </template>
        1. 无损
        <br />
        2. 文件体积大
        <span class="text-red">(约为jpg的5倍)</span>
        <br />
        3. 编码速度最慢
        <br />
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="Webp">webp</n-radio>
        </template>
        1. 无损
        <br />
        2. 文件体积大
        <span class="text-red">(约为jpg的4倍)</span>
        <br />
        3. 宽高的上限为16383
        <span class="text-red">(某些条漫可能会超过这个上限导致报错)</span>
        <br />
        4. 编码速度较慢
        <br />
      </n-tooltip>
    </n-radio-group>

    <span class="font-bold mt-2">下载目录格式</span>
    <n-tooltip placement="top" trigger="hover" :width="550">
      <div>
        可以用斜杠
        <span class="rounded bg-gray-500 px-1 text-white">/</span>
        来分隔目录层级
      </div>
      <div class="text-orange">至少要有两个层级，最后一层存放章节元数据，倒数第二层存放漫画元数据</div>
      <div class="font-semibold mt-2">可用字段：</div>
      <div class="grid grid-cols-2">
        <div>
          <span class="rounded bg-gray-500 px-1">comic_id</span>
          <span class="ml-2">漫画ID</span>
        </div>
        <div>
          <span class="rounded bg-gray-500 px-1">chapter_id</span>
          <span class="ml-2">章节ID</span>
        </div>
        <div>
          <span class="rounded bg-gray-500 px-1">comic_title</span>
          <span class="ml-2">漫画标题</span>
        </div>
        <div>
          <span class="rounded bg-gray-500 px-1">chapter_title</span>
          <span class="ml-2">章节标题</span>
        </div>
        <div>
          <span class="rounded bg-gray-500 px-1">author</span>
          <span class="ml-2">作者</span>
        </div>
        <div>
          <span class="rounded bg-gray-500 px-1">order</span>
          <span class="ml-2">章节在漫画里对应的序号</span>
        </div>
      </div>
      <div class="font-semibold mt-2">例如格式</div>
      <div class="bg-gray-200 rounded-md p-1 text-black w-fit">
        {author}/[{author}] {comic_title}({comic_id})/{order} - {chapter_title}
      </div>
      <div class="font-semibold">下载《蓦然回首》第1话会产生三层文件夹，分别是</div>
      <div class="flex gap-1 text-black">
        <span class="bg-gray-200 rounded-md px-2 w-fit">藤本树, 藤本タツキ</span>
        <span class="rounded bg-gray-500 px-1 text-white">/</span>
        <span class="bg-gray-200 rounded-md px-2 w-fit">[藤本树, 藤本タツキ] 蓦然回首(384524)</span>
        <span class="rounded bg-gray-500 px-1 text-white">/</span>
        <span class="bg-gray-200 rounded-md px-2 w-fit">1 - 第1话</span>
      </div>
      <template #trigger>
        <n-input
          v-model:value="dirFmt"
          size="small"
          @blur="store.config.dirFmt = dirFmt"
          @keydown.enter="store.config.dirFmt = dirFmt" />
      </template>
    </n-tooltip>

    <span class="font-bold mt-2">章节归档</span>
    <n-radio-group v-model:value="store.config.chapterArchiveFormat">
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="None">不打包</n-radio>
        </template>
        下载完成后保留章节目录，不做额外处理。
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="Zip">打包为 .zip</n-radio>
        </template>
        下载完成后把章节目录（含图片与章节元数据）打包为 <span class="rounded bg-gray-500 px-1 text-white">.zip</span>，
        再删除原目录；导出 PDF / CBZ 时会自动解压。
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover">
        <template #trigger>
          <n-radio value="Cbz">打包为 .cbz</n-radio>
        </template>
        下载完成后把章节目录打包为 <span class="rounded bg-gray-500 px-1 text-white">.cbz</span>（漫画阅读器约定格式）；
        适合只在本地用阅读器查看的场景。
      </n-tooltip>
    </n-radio-group>

    <span class="font-bold mt-2">缺失图片容忍</span>
    <div class="flex items-center gap-2">
      <n-tooltip placement="top" trigger="hover" :width="350">
        <template #trigger>
          <n-input-number
            v-model:value="missingImageThreshold"
            size="small"
            :min="0"
            :max="9999"
            :show-button="false"
            placeholder="缺失图片容忍阈值"
            @blur="store.config.missingImageThreshold = missingImageThreshold"
            @keydown.enter="store.config.missingImageThreshold = missingImageThreshold" />
        </template>
        <div>
          当一个章节下载结束时，若缺失的图片数 <span class="rounded bg-gray-500 px-1 text-white">≤</span> 此阈值，则视为下载成功（仅在日志中告警），不会让整章作废。
        </div>
        <div class="text-orange mt-1">
          设为 <span class="rounded bg-gray-500 px-1 text-white">0</span> 时维持上游原行为：只要缺一张就整章失败，需要手动重试整章。
        </div>
        <div class="text-gray-500 mt-1">
          失败的图片索引会写入日志（搜索 <span class="rounded bg-gray-500 px-1 text-white">chapter-download-warning</span> 或 <span class="rounded bg-gray-500 px-1 text-white">chapter-download-failure</span>），便于手动补图。
        </div>
      </n-tooltip>
      <span class="text-gray-500">张</span>
    </div>

    <span class="font-bold mt-2">中文归一化</span>
    <n-radio-group v-model:value="store.config.chineseNormalization">
      <n-tooltip placement="top" trigger="hover" :width="380">
        <template #trigger>
          <n-radio value="None">不转换</n-radio>
        </template>
        保持从网站拿到的原文（简中、繁中、日文、韩文等）落地到磁盘目录。
        <div class="text-orange mt-1">同一本漫画在不同登录语言下会被生成不同目录。</div>
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover" :width="380">
        <template #trigger>
          <n-radio value="ToSimplified">转为简体</n-radio>
        </template>
        <span class="rounded bg-gray-500 px-1 text-white">默认</span>。把繁中、日文（汉字部分）转为简体再创建目录，避免同一本漫画因为网站返回的语言不同被开成多个目录。
        <div class="text-gray-500 mt-1">韩文（Hangul）、日文假名、英文、数字、标点不会被 OpenCC 连带改写。</div>
      </n-tooltip>
      <n-tooltip placement="top" trigger="hover" :width="380">
        <template #trigger>
          <n-radio value="ToTraditional">转为繁体</n-radio>
        </template>
        把简中、日文（汉字部分）转为繁体再创建目录。
        <div class="text-gray-500 mt-1">韩文（Hangul）、日文假名、英文、数字、标点不会被 OpenCC 连带改写。</div>
      </n-tooltip>
    </n-radio-group>

    <span class="font-bold mt-2">其他</span>
    <n-checkbox class="w-fit" v-model:checked="store.config.shouldDownloadCover">下载封面</n-checkbox>
  </div>
</template>
