pub use grappler_core as core;
pub use grappler_macros::hook;

#[cfg(target_os = "windows")]
pub fn write_at_offset(data: &[u8], offset: usize) -> Result<(), Box<dyn std::error::Error>> {
    use core::poggers::structures::process::Process;
    use core::poggers::structures::{
        process::implement::utils::ProcessUtils, protections::Protections,
    };
    use core::poggers::traits::Mem;
    let process = Process::this_process();
    let module = process.get_base_module()?;
    let base_address = module.get_base_address();

    // Reject writes that fall outside the base module rather than letting an
    // out-of-range `offset` scribble over arbitrary process memory.
    let write_end = offset
        .checked_add(data.len())
        .ok_or("write_at_offset: offset + data length overflows usize")?;
    if write_end > module.get_size() {
        return Err(format!(
            "write_at_offset: write of {} bytes at offset 0x{:X} exceeds module size 0x{:X}",
            data.len(),
            offset,
            module.get_size()
        )
        .into());
    }
    let address = base_address
        .checked_add(offset)
        .ok_or("write_at_offset: base address + offset overflows usize")?;

    unsafe {
        core::info!(
            "Writing {} bytes to address 0x{:X?} (base 0x{:X?} + 0x{:X?})",
            data.len(),
            address,
            base_address,
            offset
        );
        // Make the region writable, then restore its *original* protection on
        // every exit path. Capturing the old value (instead of hard-coding
        // ExecuteRead) means a failed write can never leave a writable +
        // executable page behind, and a non-executable region stays that way.
        let original =
            process.alter_protection(address, data.len(), Protections::ExecuteReadWrite)?;
        let write_result = process.write_raw(address, data);
        let restore_result = process.alter_protection(address, data.len(), original);
        write_result?;
        restore_result?;
    }

    Ok(())
}
