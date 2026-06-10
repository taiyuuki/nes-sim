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
            audioContext.value = new AudioContext({ sampleRate: 44100 })
        }

        // 恢复 AudioContext（浏览器可能暂停它）
        if (audioContext.value.state === 'suspended') {
            await audioContext.value.resume()
        }

        // 加载 AudioWorklet module
        if (!workletNode.value) {
            try {
                await audioContext.value.audioWorklet.addModule(workletUrl)
                console.log('[AudioWorklet] Module 加载成功')

                // 创建 AudioWorkletNode
                workletNode.value = new AudioWorkletNode(audioContext.value, 'audio-processor')

                // 创建增益节点用于音量控制
                gainNode = audioContext.value.createGain()
                gainNode.gain.value = volume.value

                // 连接节点：worklet -> gain -> destination
                workletNode.value.connect(gainNode)
                gainNode.connect(audioContext.value.destination)

                console.log('[AudioWorklet] 音频节点已连接')
            }
            catch(error) {
                console.error('[AudioWorklet] 初始化失败:', error)
            }
        }
    }

    // 播放音频样本
    function playAudioSamples(audioData: AudioData) {
        if (!enabled.value || !workletNode.value || audioData.samples.length === 0) {
            return
        }

        const samples = new Float32Array(audioData.samples)

        // 发送音频数据到 AudioWorklet
        workletNode.value.port.postMessage({
            type: 'audio-data',
            data: samples.buffer,
        }, [samples.buffer])

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
            console.log('[AudioWorklet] 音频事件监听已设置')
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
