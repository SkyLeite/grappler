pub use grappler_core as core;
use grappler_core::poggers::structures::process::implement::utils::ProcessUtils;
pub use grappler_macros::hook;

pub fn write_at_offset(data: &[u8], offset: usize) -> Result<(), Box<dyn std::error::Error>> {
    use core::poggers::structures::process::Process;
    use core::poggers::traits::Mem;
    let process = Process::this_process();
    let module = process.get_base_module()?;
    let base_address = module.get_base_address();
    let address = base_address.add(offset);

    unsafe {
        core::info!(
            "Writing {} bytes to address 0x{:X?} (base 0x{:X?} + 0x{:X?})",
            data.len(),
            address,
            base_address,
            offset
        );
        process.alter_protection(address, data.len(), Protections::ExecuteReadWrite)?;
        process.write_raw(address, &data)?;
        process.alter_protection(address, data.len(), Protections::ExecuteRead)?;
    }
}
