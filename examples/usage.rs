//! Compile-checked examples of grappler's hooking macros. These mirror the
//! `using-grappler` usage skill. `cargo build --examples` type-checks them; the
//! addresses/signatures are illustrative, so the install calls are guarded and
//! the example is not meant to be run.
use grappler::{hook, mid_hook, Registers};

struct Ctx;

// Entry hook: replace `draw`, and call the original from the replacement.
#[hook(signature = "48 89 5C 24 ?? 57")]
fn draw(ctx: *mut Ctx, flags: u32) -> bool {
    draw.call_original(ctx, flags)
}

// Passthrough: a body-less hook that just forwards to the original.
#[hook(offset = 0x1A30)]
fn draw_passthrough(ctx: *mut Ctx, flags: u32) -> bool;

// Mid hook (resume): read/modify registers, then continue the original code.
// Register fields are arch-specific (rax on x86_64, eax on x86).
#[mid_hook(offset = 0x1C40)]
fn on_tick(regs: &mut Registers) {
    #[cfg(target_arch = "x86_64")]
    {
        regs.rax = 0;
    }
    #[cfg(target_arch = "x86")]
    {
        regs.eax = 0;
    }
}

const SKIP_ADDR: usize = 0x4000;

// Mid hook (redirect): return `original` to resume, or another address to jump.
#[mid_hook(signature = "8B 45 ?? 85 C0")]
fn gate(regs: &mut Registers, original: usize) -> usize {
    #[cfg(target_arch = "x86_64")]
    let cond = regs.rcx == 0;
    #[cfg(target_arch = "x86")]
    let cond = regs.ecx == 0;

    if cond {
        SKIP_ADDR
    } else {
        original
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Guarded: type-checks installation without hooking this example binary.
    if std::env::args().any(|a| a == "--never") {
        draw.initialize()?;
        draw_passthrough.initialize()?;
        on_tick.initialize()?;
        gate.initialize()?;
        grappler::write_at_offset(&[0x90, 0x90], 0x1234)?;
    }
    Ok(())
}
