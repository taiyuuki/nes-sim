import { onMounted, onUnmounted, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type { AudioData } from '../types'

// AudioWorklet 处理器 URL（public 目录中的静态资源）
const workletUrl = '/audio-processor.js'

export function useAudioPlayer() {
    const audioContext = ref<AudioContext | null>(null)
    const workletNode = ref<AudioWorkletNode | null>(null)
    const volume = ref(0.5)
    const enabled = ref(false)
    const isPlaying = ref(false)

    let unlisten: (() => void) | null = null
    let gainNode: GainNode | null = null

    // 从 localStorage 读取状态
    onMounted(() => {
        const saved = localStorage.getItem('audio-enabled')
        if (saved === 'true') {
            enabled.value = true
        }
        const savedVolume = localStorage.getItem('audio-volume')
        if (savedVolume) {
            volume.value = Number.parseFloat(savedVolume)
        }

        // 初始化 AudioContext（如果启用）
        if (enabled.value) {
            initAudioContext()
        }

        // 监听后端音频事件
        setupAudioListener()
    })

    // 初始化 AudioContext
    async function initAudioContext() {
        if (!audioContext.value) {

            // 不强制设置采样率，使用系统默认值
            audioContext.value = new AudioContext()
        }

        // 恢复 AudioContext（浏览器可能暂停它）
        if (audioContext.value.state === 'suspended') {
            await audioContext.value.resume()
        }

        // 加载 AudioWorklet module
        if (!workletNode.value) {
            try {
                await audioContext.value.audioWorklet.addModule(workletUrl)

                // 创建 AudioWorkletNode，设置更大的 buffer size 以降低消费频率
                workletNode.value = new AudioWorkletNode(
                    audioContext.value,
                    'audio-processor',
                    {
                        processorOptions: {

                            // 设置为 4096 样本，约 93ms，减少 process 调用频率
                            bufferSize: 4096,
                        },
                    },
                )

                // 创建增益节点用于音量控制
                gainNode = audioContext.value.createGain()
                gainNode.gain.value = volume.value

                // 连接节点：worklet -> gain -> destination
                workletNode.value.connect(gainNode)
                gainNode.connect(audioContext.value.destination)

            }
            catch(error) {
                console.error('[AudioWorklet] 初始化失败:', error)
            }
        }
    }

    // 重采样函数：将音频从源采样率转换到目标采样率
    function resampleAudio(samples: Float32Array, fromRate: number, toRate: number): Float32Array {
        if (fromRate === toRate) {
            return samples
        }

        const ratio = fromRate / toRate
        const outputLength = Math.round(samples.length / ratio)
        const output = new Float32Array(outputLength)

        for (let i = 0; i < outputLength; i++) {
            const srcIndex = i * ratio
            const srcIndexLow = Math.floor(srcIndex)
            const srcIndexHigh = Math.min(srcIndexLow + 1, samples.length - 1)
            const frac = srcIndex - srcIndexLow

            // 线性插值
            output[i] = samples[srcIndexLow] * (1 - frac) + samples[srcIndexHigh] * frac
        }

        return output
    }

    // 播放音频样本
    function playAudioSamples(audioData: AudioData) {
        if (!enabled.value || !workletNode.value || audioData.samples.length === 0) {
            return
        }

        let samples: Float32Array = new Float32Array(audioData.samples)
        const targetRate = audioContext.value?.sampleRate || 44100

        // 如果采样率不匹配，进行重采样
        if (audioData.sample_rate && audioData.sample_rate !== targetRate) {
            samples = resampleAudio(samples, audioData.sample_rate, targetRate)
        }

        // 直接发送给 AudioWorklet，不要前端缓冲
        // 复制到新 buffer 避免 transfer 污染原数据
        const bufferCopy = new Float32Array(samples.length)
        bufferCopy.set(samples)

        workletNode.value.port.postMessage({
            type: 'audio-data',
            data: bufferCopy.buffer,
        }, [bufferCopy.buffer])

        isPlaying.value = true

        // 更新增益
        if (gainNode) {
            gainNode.gain.value = volume.value
        }
    }

    // 设置音频事件监听
    async function setupAudioListener() {
        if (unlisten) {
            unlisten()
        }

        try {
            unlisten = await listen<AudioData>('audio_data', event => {
                if (enabled.value) {
                    playAudioSamples(event.payload)
                }
            })
        }
        catch(error) {
            console.error('[AudioWorklet] 设置音频监听失败:', error)
        }
    }

    // 移除音频事件监听
    function removeAudioListener() {
        if (unlisten) {
            unlisten()
            unlisten = null
        }
    }

    // 启用音频
    async function enable() {
        await initAudioContext()
        enabled.value = true
    }

    // 禁用音频
    function disable() {
        enabled.value = false
        removeAudioListener()

        // 清空 AudioWorklet 队列
        if (workletNode.value) {
            workletNode.value.port.postMessage({ type: 'clear' })
        }

        // 断开连接
        if (workletNode.value) {
            workletNode.value.disconnect()
            workletNode.value = null
        }

        if (audioContext.value) {
            audioContext.value.close()
            audioContext.value = null
        }

        // 清空缓冲区
        isPlaying.value = false
        gainNode = null
    }

    // 设置音量
    function setVolume(value: number) {
        volume.value = Math.max(0, Math.min(1, value))
    }

    // 切换启用状态
    function toggle() {
        if (enabled.value) {
            disable()
        }
        else {
            enable()
        }
    }

    // 监听状态变化
    watch(enabled, value => {
        localStorage.setItem('audio-enabled', String(value))
        if (value) {
            setupAudioListener()
        }
        else {
            removeAudioListener()
        }
    })

    watch(volume, value => {
        localStorage.setItem('audio-volume', String(value))
    })

    // 清理
    onUnmounted(() => {
        disable()
        removeAudioListener()
    })

    return {
        enabled,
        volume,
        isPlaying,
        enable,
        disable,
        toggle,
        setVolume,
    }
}
