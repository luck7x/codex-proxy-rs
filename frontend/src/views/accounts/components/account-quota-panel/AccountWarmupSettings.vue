<script setup lang="ts">
import { ZButton, ZEmpty, ZInput, ZNotification, ZSegmented, ZTag } from '@codex-proxy/ui'
import { computed, shallowRef, watch } from 'vue'
import { getAccountWarmup, setAccountWarmup } from '@/api'
import { useAsyncAction } from '@/composables/useAsyncAction'
import { useRequestState } from '@/composables/useRequestState'

type WarmupMode = 'global' | 'custom' | 'disabled'

const props = defineProps<{ accountId: string }>()

const MAX_TIMES = 8
const modeOptions = [{ label: '跟随全局', value: 'global' }, { label: '自定义', value: 'custom' }, { label: '关闭', value: 'disabled' }]
const mode = shallowRef<WarmupMode>('global')
const times = shallowRef<string[]>([])
const newTime = shallowRef('')
const savedTimes = shallowRef<string[] | null | undefined>()
const request = useRequestState()
const saveAction = useAsyncAction()

const selectedTimes = computed(() => {
  if (mode.value === 'global')
    return null
  return mode.value === 'disabled' ? [] : times.value
})
const busy = computed(() => savedTimes.value === undefined || saveAction.loading.value)
const dirty = computed(() => JSON.stringify(selectedTimes.value) !== JSON.stringify(savedTimes.value))
const canSave = computed(() => dirty.value && (mode.value !== 'custom' || times.value.length > 0))
const description = computed(() => {
  switch (mode.value) {
    case 'global': return '按设置页的执行时间预激活'
    case 'custom': return '只在这些时间预激活，需要失败补发时多加一个时间'
    default: return '该账号不参与预激活'
  }
})

function applySaved(saved: string[] | null) {
  savedTimes.value = saved
  times.value = saved ?? []
  if (saved === null)
    mode.value = 'global'
  else
    mode.value = saved.length > 0 ? 'custom' : 'disabled'
}

async function load() {
  const requestId = request.start()
  try {
    const result = await getAccountWarmup({ accountId: props.accountId }, { signal: request.signal, silent: true })
    if (request.isCurrent(requestId))
      applySaved(result.times)
  }
  catch (error) {
    request.fail(requestId, error)
  }
  finally {
    request.finish(requestId)
  }
}

function addTime() {
  if (newTime.value && !times.value.includes(newTime.value))
    times.value = [...times.value, newTime.value].sort()
  newTime.value = ''
}

function removeTime(time: string) {
  times.value = times.value.filter(item => item !== time)
}

async function save() {
  const accountId = props.accountId
  await saveAction.run(async () => {
    const result = await setAccountWarmup({ accountId, times: selectedTimes.value })
    if (props.accountId === accountId) {
      applySaved(result.times)
      ZNotification.success({ message: '预激活设置已保存' })
    }
  })
}

watch(() => props.accountId, () => {
  request.invalidate()
  savedTimes.value = undefined
  mode.value = 'global'
  times.value = []
  newTime.value = ''
  void load()
}, { immediate: true })
</script>

<template>
  <section class="grid gap-2 rounded-cp bg-cp-fill-quaternary px-4 py-3" aria-label="账号预激活">
    <h3 class="m-0 text-cp-sm font-heavy text-cp-text">
      预激活
    </h3>
    <ZEmpty v-if="request.error.value" title="预激活设置加载失败" :description="request.error.value" size="small" surface="none" role="alert">
      <template #action>
        <ZButton size="small" variant="plain" :loading="request.loading.value" @click="load">
          重试
        </ZButton>
      </template>
    </ZEmpty>
    <template v-else>
      <ZSegmented v-model="mode" aria-label="预激活方式" :options="modeOptions" :disabled="busy" />
      <p class="m-0 text-cp-xs text-cp-text-secondary">
        {{ description }}
      </p>
      <template v-if="mode === 'custom'">
        <div v-if="times.length > 0" class="flex flex-wrap gap-1.5">
          <ZTag v-for="time in times" :key="time" size="small" :closable="!busy" @close="removeTime(time)">
            {{ time }}
          </ZTag>
        </div>
        <div v-if="times.length < MAX_TIMES" class="flex items-center gap-2">
          <ZInput v-model="newTime" aria-label="新增预激活时间" type="time" class="max-w-36" :disabled="busy" />
          <ZButton size="small" variant="plain" :disabled="busy || !newTime" @click="addTime">
            添加
          </ZButton>
        </div>
      </template>
      <div class="flex justify-end">
        <ZButton size="small" :loading="saveAction.loading.value" :disabled="!canSave" @click="save">
          保存
        </ZButton>
      </div>
    </template>
  </section>
</template>
