
class AudioProcessor extends AudioWorkletProcessor {
    constructor() {
        super()
        this.audioQueue = []
        this.currentBuffer = null
        this.currentIndex = 0
        this.totalQueued = 0

        this.targetBuffer = 2205 // 50ms，达到即开始播放
        this.maxBuffer = 4410 // 100ms 硬上限

        this.isPlaying = false

        this.port.onmessage = event => {
            const { type, data } = event.data

            switch (type) {
                case 'audio-data': {
                    let samples = new Float32Array(data)

                    // ── 队列上限管理：丢弃最旧数据 ──
                    while (this.totalQueued + samples.length > this.maxBuffer
                        && this.audioQueue.length > 0) {
                        const old = this.audioQueue.shift()
                        this.totalQueued -= old.length
                    }

                    // 单个包就超限，只保留最新的 maxBuffer
                    if (samples.length > this.maxBuffer) {
                        samples = samples.subarray(samples.length - this.maxBuffer)
                        this.totalQueued = 0
                        this.audioQueue = []
                        this.currentBuffer = null
                        this.currentIndex = 0
                    }

                    this.audioQueue.push(samples)
                    this.totalQueued += samples.length

                    if (!this.isPlaying && this.totalQueued >= this.targetBuffer) {
                        this.isPlaying = true
                    }
                    break
                }
                case 'clear':
                    this.audioQueue = []
                    this.currentBuffer = null
                    this.currentIndex = 0
                    this.totalQueued = 0
                    this.isPlaying = false
                    break
            }
        }
    }

    process(inputs, outputs) {
        const output = outputs[0]
        if (!output || output.length === 0) return true

        const channel = output[0]
        const length = channel.length

        // 未启动或完全无数据：输出完整静音
        if (!this.isPlaying || this.totalQueued === 0) {
            for (let i = 0; i < length; i++) channel[i] = 0

            return true
        }

        // ── 核心：必须写满整个 length，不能 break ──
        for (let i = 0; i < length; i++) {
            if (this.currentBuffer && this.currentIndex < this.currentBuffer.length) {
                channel[i] = this.currentBuffer[this.currentIndex]
                this.currentIndex++
                this.totalQueued--
            }
            else {

                // 切到下一个 buffer
                this.currentBuffer = this.audioQueue.length > 0 ? this.audioQueue.shift() : null
                this.currentIndex = 0

                if (this.currentBuffer) {
                    channel[i] = this.currentBuffer[this.currentIndex]
                    this.currentIndex++
                    this.totalQueued--
                }
                else {

                    // 队列空了，补静音并标记停止
                    channel[i] = 0
                    this.isPlaying = false
                }
            }
        }

        return true
    }
}

registerProcessor('audio-processor', AudioProcessor)
