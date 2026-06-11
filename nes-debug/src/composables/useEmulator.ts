import { onMounted, onUnmounted, ref, shallowRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DebugInfo, DisasmResult, FrameData, RunFrameResult } from '../types'

const debugInfo = shallowRef<DebugInfo | null>(null)
const disasmResult = shallowRef<DisasmResult | null>(null)
const frameData = shallowRef<FrameData | null>(null)
const paused = ref(true)
const running = ref(false)
const romPath = ref('')
const error = ref('')
const tick = ref(0)

// Controller bits: A=0x01, B=0x02, Select=0x04, Start=0x08, Up=0x10, Down=0x20, Left=0x40, Right=0x80
const KEY_MAP: Record<string, number> = {
    KeyX:       0x01, // A
    KeyZ:       0x02, // B
    ShiftRight: 0x04, // Select
    Enter:      0x08, // Start
    ArrowUp:    0x10,
    ArrowDown:  0x20,
    ArrowLeft:  0x40,
    ArrowRight: 0x80,
}

const pressedKeys = new Set<string>()
let loopId = 0

export function useEmulator() {
    function getControllerBits(): number {
        let bits = 0
        for (const key of pressedKeys) {
            if (KEY_MAP[key]) bits |= KEY_MAP[key]
        }

        return bits
    }

    async function loadRom(path: string) {
        try {
            error.value = ''
            await invoke('load_rom', { path })
            romPath.value = path
            running.value = true
            await invoke('set_paused', { paused: true })
            paused.value = true
            await refresh()
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function reset() {
        try {
            await invoke('reset')
            await refresh()
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function stepFrame() {
        if (!paused.value) return
        try {
            const info = await invoke<DebugInfo>('step_frame')
            debugInfo.value = info
            await fetchFrame()
            await fetchDisasm()
            tick.value++
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function stepInstruction() {
        if (!paused.value) return
        try {
            const info = await invoke<DebugInfo>('step_instruction')
            debugInfo.value = info
            await fetchFrame()
            await fetchDisasm()
            tick.value++
        }
        catch(e) {
            error.value = String(e)
        }
    }

    function togglePause() {
        if (paused.value) {
            resume()
        }
        else {
            pause()
        }
    }

    async function pause() {
        stopLoop()
        paused.value = true
        await invoke('set_paused', { paused: true }).catch(() => {})
        await refresh()
    }

    async function resume() {
        paused.value = false
        await invoke('set_paused', { paused: false }).catch(() => {})
        startLoop()
    }

    function startLoop() {
        stopLoop()
        const id = ++loopId
        let lastTick = performance.now()
        const loop = async() => {
            if (loopId !== id || paused.value) return
            try {
                const controller = getControllerBits()
                const result = await invoke<RunFrameResult>('run_frame', { controller })
                debugInfo.value = result.debug_info
                frameData.value = result.frame

                const now = performance.now()
                if (now - lastTick > 500) {
                    lastTick = now
                    tick.value++
                }

                if (result.debug_info.paused) {
                    paused.value = true
                    tick.value++

                    return
                }
                requestAnimationFrame(loop)
            }
            catch(e) {
                error.value = String(e)
                paused.value = true
            }
        }
        requestAnimationFrame(loop)
    }

    function stopLoop() {
        loopId++
    }

    async function addBreakpoint(type: string, value?: number) {
        try {
            await invoke('add_breakpoint', { bpDef: { type, value } })
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function removeBreakpoint(type: string, value?: number) {
        try {
            await invoke('remove_breakpoint', { bpDef: { type, value } })
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function refresh() {
        try {
            const info = await invoke<DebugInfo>('get_debug_info')
            debugInfo.value = info
            await fetchFrame()
            await fetchDisasm()
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function fetchFrame() {
        try {
            const frame = await invoke<FrameData>('get_frame')
            frameData.value = frame
        }
        catch(e) {
            error.value = String(e)
        }
    }

    async function fetchDisasm() {
        try {
            const result = await invoke<DisasmResult>('disassemble', { rows: 10 })
            disasmResult.value = result
        }
        catch {
            disasmResult.value = null
        }
    }

    // Keyboard handling
    function onKeyDown(e: KeyboardEvent) {
        pressedKeys.add(e.code)

        // Debug shortcuts (only when input not focused)
        if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return

        switch (e.code) {
            case 'F5':
                e.preventDefault()
                togglePause()
                break
            case 'F6':
                e.preventDefault()
                stepFrame()
                break
            case 'F7':
                e.preventDefault()
                stepInstruction()
                break
            case 'KeyR':
                if (!e.ctrlKey && !e.metaKey) {
                    reset()
                }
                break
        }
    }

    function onKeyUp(e: KeyboardEvent) {
        pressedKeys.delete(e.code)
    }

    onMounted(() => {
        window.addEventListener('keydown', onKeyDown)
        window.addEventListener('keyup', onKeyUp)
    })

    onUnmounted(() => {
        window.removeEventListener('keydown', onKeyDown)
        window.removeEventListener('keyup', onKeyUp)
        stopLoop()
    })

    return {
        debugInfo,
        disasmResult,
        frameData,
        paused,
        running,
        romPath,
        error,
        tick,
        loadRom,
        reset,
        stepFrame,
        stepInstruction,
        togglePause,
        pause,
        resume,
        addBreakpoint,
        removeBreakpoint,
        refresh,
    }
}
