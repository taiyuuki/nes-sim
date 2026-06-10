// AudioWorkletProcessor 用于处理音频流
class AudioProcessor extends AudioWorkletProcessor {
    constructor() {
        super()
        console.log('[AudioProcessor] 初始化')

        // 音频队列
        this.audioQueue = []

        // 当前播放缓冲区
        this.currentBuffer = null
        this.currentBufferIndex = 0

        // 监听来自主线程的消息
        this.port.onmessage = event => {
            const { type, data } = event.data

            switch (type) {
                case 'audio-data':

                    // 将音频数据加入队列
                    this.audioQueue.push(new Float32Array(data))
                    break
                case 'clear':

                    // 清空音频队列
                    this.audioQueue = []
                    this.currentBuffer = null
                    this.currentBufferIndex = 0
                    console.log('[AudioProcessor] 队列已清空')
                    break
            }
        }
    }

    process(inputs, outputs) {

        // 获取输出缓冲区
        const output = outputs[0]
        if (!output || output.length === 0) {
            return true
        }

        const channel = output[0]
        const length = channel.length

        // 填充输出缓冲区
        for (let i = 0; i < length; i++) {
            if (this.currentBuffer && this.currentBufferIndex < this.currentBuffer.length) {

                // 从当前缓冲区读取
                channel[i] = this.currentBuffer[this.currentBufferIndex]
                this.currentBufferIndex++
            }
            else {

                // 当前缓冲区已用完，尝试获取下一个
                this.currentBuffer = null
                this.currentBufferIndex = 0

                if (this.audioQueue.length > 0) {
                    this.currentBuffer = this.audioQueue.shift()
                    channel[i] = this.currentBuffer[this.currentBufferIndex]
                    this.currentBufferIndex++
                }
                else {

                    // 没有更多音频数据，填充静音
                    channel[i] = 0
                }
            }
        }

        return true
    }
}

// 注册处理器
registerProcessor('audio-processor', AudioProcessor)
