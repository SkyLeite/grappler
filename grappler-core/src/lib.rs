pub use ilhook;

// Mid-hooks use the ilhook module and register-context struct matching the
// target architecture. `ilhook_arch` and `Registers` are what the #[mid_hook]
// generated code and user handlers refer to.
#[cfg(target_arch = "x86_64")]
pub use ilhook::x64 as ilhook_arch;
#[cfg(target_arch = "x86_64")]
pub use ilhook::x64::Registers;

#[cfg(target_arch = "x86")]
pub use ilhook::x86 as ilhook_arch;
#[cfg(target_arch = "x86")]
pub use ilhook::x86::Registers;
pub use poggers;
pub use retour::static_detour;
pub use skidscan::Signature;
pub use tracing::{info, trace};
