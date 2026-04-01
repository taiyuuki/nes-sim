# MMC3 IRQ Notes

## Current status

The current MMC3 implementation is good enough to boot and render `SuperContra(U).nes`
correctly through the stage 1 boss scene, including the split-screen section that was
previously jittering.

Two fixes were required:

1. Filter A12 rises using a low-time requirement.
2. Suppress duplicate IRQ clocks that occur within the same scanline-sized window.

The second rule is intentionally documented as a compatibility guard, not as the final
hardware model.

## Why the spacing guard exists

Tracing the boss scene in `SuperContra(U).nes` showed accepted `a12-clock` events only
`40` PPU cycles apart while IRQs were enabled. That is too short to represent separate
scanlines and caused the lower split to jitter.

The current code therefore rejects accepted MMC3 IRQ clocks that occur within
`IRQ_CLOCK_MIN_SPACING_PPU_CYCLES` of the previous accepted clock.

## Follow-up target

Long-term, the spacing guard should be replaced by a more exact model of which PPU bus
accesses are externally visible to MMC3 and therefore eligible to clock the scanline
counter.

## Regression checklist

Automated:

- Run `cargo test`.
- Run `cargo test cartridge::tests::mmc3 -- --nocapture` when iterating on MMC3 logic.
- Run `cargo test supercontra_mmc3_rom_boot_frame_matches_reference_hash -- --ignored`.

Manual:

- Boot `roms/mmc3/SuperContra(U).nes`.
- Verify stage 1 scroll is stable before the boss.
- Verify the stage 1 boss split screen does not jitter, disappear, or jump.
- Verify the lower status/background region stays stable while the helicopter boss is on screen.
- Verify the screen returns to normal after the boss is defeated.
- Verify save/load still works in an MMC3 game after any MMC3 state-format change.

Tracing:

- Use `NES_TRACE_MMC3=1` to log key MMC3 events.
- Add `NES_TRACE_MMC3_VERBOSE=1` only when investigating filtered A12 rises; the log becomes very large.
