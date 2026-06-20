extern crate inflector;

use darling::{ast::NestedMeta, Error, FromMeta};
use inflector::Inflector;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::ItemFn;

#[derive(Debug, FromMeta)]
struct MacroArgs {
    signature: Option<String>,
    offset: Option<usize>,
    /// Module to scan for `signature`. Defaults to the current executable;
    /// set it to target a function that lives in a loaded library, e.g.
    /// `#[hook(signature = "...", module = "d3d11.dll")]`.
    module: Option<String>,
}

/// Render an `Option<T>` as the tokens `Some(value)` or `None`, used to embed
/// the configured signature/offset back into the generated accessors.
fn option_tokens<T: quote::ToTokens>(opt: Option<T>) -> proc_macro2::TokenStream {
    match opt {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    }
}

/// Validate that the attribute names a usable address source. On failure,
/// returns `compile_error!` tokens spanned on `span`. Shared by `#[hook]` and
/// `#[mid_hook]`.
fn validate_args<T: quote::ToTokens>(
    args: &MacroArgs,
    span: &T,
) -> Result<(), proc_macro2::TokenStream> {
    let err = |msg: &str| Err(syn::Error::new_spanned(span, msg).to_compile_error());
    match (&args.signature, &args.offset) {
        (None, None) => return err("a hook requires either a `signature` or an `offset` argument"),
        (Some(signature), _) => {
            if signature.is_empty() {
                return err("Signature cannot be empty");
            }
            if signature.replace([' ', '?'], "").is_empty() {
                return err("Signature must contain at least one known byte");
            }
        }
        (None, Some(_)) => {}
    }
    Ok(())
}

/// Build an expression that resolves the target address as a `usize`. Intended
/// to be interpolated inside `unsafe { ... }` within a function returning a
/// `Result` (it uses `?`). Assumes `validate_args` has already passed. Shared
/// by `#[hook]` and `#[mid_hook]`.
fn resolve_address_tokens(args: &MacroArgs) -> proc_macro2::TokenStream {
    if let Some(ref signature) = args.signature {
        let resolve_module = if let Some(ref module) = args.module {
            quote! { let module_name = #module; }
        } else {
            quote! {
                let exe = std::env::current_exe()?;
                let module_name = exe
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("hook: current executable has no valid UTF-8 file name")?;
            }
        };

        quote! {
            #resolve_module
            // skidscan's errors don't implement std::error::Error, so map them
            // to a string before propagating. scan_module yields a *mut u8;
            // normalise to usize so both resolution paths return the same type.
            grappler::core::Signature::from_str(#signature)
                .map_err(|e| format!("hook: invalid signature {:?}: {:?}", #signature, e))?
                .scan_module(module_name)
                .map_err(|e| format!("hook: signature scan failed: {:?}", e))? as usize
        }
    } else if let Some(ref offset) = args.offset {
        quote! {
            let this_proc = grappler::core::poggers::structures::process::Process::this_process();
            let base_module = this_proc.get_base_module()?;
            let base_addr = base_module.get_base_address();
            base_addr
                .checked_add(#offset)
                .filter(|addr| *addr < base_module.get_end_address())
                .ok_or("hook: offset resolves outside the base module")?
        }
    } else {
        unreachable!("a missing signature and offset was rejected by validate_args");
    }
}

#[proc_macro_attribute]
pub fn hook(args: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(Error::from(e).write_errors());
        }
    };

    // Accept either a normal function with a body, or a body-less signature
    // declaration `fn foo(..) -> ..;` (parsed as a ForeignItemFn). The latter
    // becomes a passthrough hook that just forwards to the original. When the
    // item parses as neither, surface the more descriptive ItemFn error.
    let (fn_vis, sig, original_fn) = match syn::parse::<ItemFn>(item.clone()) {
        Ok(item_fn) => {
            // Re-emit the user's function under a private name so the detour
            // closure can call it.
            let mut original = item_fn.clone();
            original.sig.ident = format_ident!("__{}_original", item_fn.sig.ident);
            (item_fn.vis, item_fn.sig, Some(original))
        }
        Err(item_err) => match syn::parse::<syn::ForeignItemFn>(item) {
            Ok(foreign) => (foreign.vis, foreign.sig, None),
            Err(_) => return item_err.to_compile_error().into(),
        },
    };
    let passthrough = original_fn.is_none();

    let args = match MacroArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.write_errors());
        }
    };

    // Validate the attribute up front and surface problems as proper spanned
    // compiler errors rather than panicking (a macro panic produces an opaque
    // "custom attribute panicked" diagnostic with no useful span).
    if sig
        .inputs
        .iter()
        .any(|arg| matches!(arg, syn::FnArg::Receiver(_)))
    {
        return syn::Error::new_spanned(
            &sig,
            "#[hook] cannot be applied to a function that takes `self`",
        )
        .to_compile_error()
        .into();
    }

    if let Err(err) = validate_args(&args, &sig) {
        return err.into();
    }

    let name = &sig.ident;
    let inputs = &sig.inputs;
    let output = &sig.output;
    let mod_name = format_ident!("__{}", name);
    let struct_name = format_ident!("__{}Hook", name.to_string().to_table_case());
    let spanned_struct = quote! {
        #mod_name::#struct_name
    };

    let retour_fn_name = format_ident!("__{}Retour", name);
    let retour_fn_abi = &sig.abi;

    // For a normal hook the original body is re-emitted under a private name;
    // a passthrough has no body to emit.
    let original_fn = match original_fn {
        Some(original) => quote! { #original },
        None => quote! {},
    };

    // Receivers (`self`) were rejected above, so every remaining argument is
    // a typed parameter.
    let input_types: Vec<_> = inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => Some(&*pat_type.ty),
            syn::FnArg::Receiver(_) => None,
        })
        .collect();

    // Synthesize a positional name for every parameter instead of reusing the
    // source patterns. The detour closure and call_original forward arguments
    // by position, so binding `__arg0..__argN` works for parameters the source
    // writes as `_`, `mut x`, or a destructuring pattern — cases where reading
    // the original `Pat::Ident` would silently drop the argument and leave the
    // generated closure with fewer parameters than the detour signature.
    let input_names: Vec<_> = (0..input_types.len())
        .map(|i| format_ident!("__arg{}", i))
        .collect();

    // The detour closure forwards its arguments to the original. A normal hook
    // calls the user's renamed body; a passthrough calls the real original
    // through the detour trampoline.
    let trace_label = name.to_string();
    let forward_call = if passthrough {
        quote! { #retour_fn_name.call(#(#input_names),*) }
    } else {
        let original_name = format_ident!("__{}_original", name);
        quote! { #original_name(#(#input_names),*) }
    };

    let fn_sig = quote! {
        fn(#(#input_types),*) #output
    };

    let address_fn = resolve_address_tokens(&args);

    let maybe_signature = option_tokens(args.signature.as_deref());
    let maybe_offset = option_tokens(args.offset);

    let tokens = quote! {
        #original_fn

        #[doc(hidden)]
        mod #mod_name {
            use std::str::FromStr;
            use grappler::core::poggers::structures::process::implement::utils::ProcessUtils as _;
            use super::*;

            grappler::core::static_detour! {
                pub static #retour_fn_name: #retour_fn_abi #fn_sig;
            }

            pub struct #struct_name;

            impl #struct_name {
                pub fn initialize(&self) -> Result<(), Box<dyn std::error::Error>> {
                    let address = unsafe {
                        #address_fn
                    };

                    self.initialize_ptr(address as *mut u8)
                }

                pub fn initialize_ptr(&self, ptr: *mut u8) -> Result<(), Box<dyn std::error::Error>> {
                    if ptr.is_null() {
                        return Err("hook: cannot install a hook over a null pointer".into());
                    }

                    let pointer = unsafe { std::mem::transmute(ptr) };

                    unsafe {
                        #retour_fn_name.initialize(pointer, |#(#input_names),*| {
                            grappler::core::trace!("Executing hook: {}", #trace_label);
                            #forward_call
                        })?.enable()?;
                    }

                    Ok(())
                }

                pub fn call_original(&self, #(#input_names: #input_types),*) #output {
                    #retour_fn_name.call(#(#input_names),*)
                }

                pub fn signature(&self) -> Option<&str> {
                    #maybe_signature
                }

                pub fn offset(&self) -> Option<usize> {
                    #maybe_offset
                }
            }
        }

        #[allow(non_upper_case_globals)] #fn_vis const #name: #spanned_struct = #spanned_struct {};
    };

    TokenStream::from(tokens)
}

/// Install a mid-function hook at an arbitrary in-body address.
///
/// Unlike `#[hook]`, the annotated function is a *handler* that receives the
/// captured CPU register state — there is no original function to call. The
/// handler's signature selects the behaviour:
///
/// * `fn(&mut grappler::Registers)` — inspect/modify registers, then resume the
///   original code.
/// * `fn(&mut grappler::Registers, original: usize) -> usize` — redirect control
///   flow: `original` is the address the original code would continue at, and
///   the returned address is jumped to instead (return `original` to resume).
///
/// ```ignore
/// // Resume after tweaking a register.
/// #[grappler::mid_hook(offset = 0x1234)]
/// fn my_hook(regs: &mut grappler::Registers) {
///     regs.rax = 1337;
/// }
///
/// // Conditionally redirect.
/// #[grappler::mid_hook(offset = 0x2000)]
/// fn my_redirect(regs: &mut grappler::Registers, original: usize) -> usize {
///     if regs.rcx == 0 { SOMEWHERE_ELSE } else { original }
/// }
///
/// my_hook.initialize()?;
/// ```
#[proc_macro_attribute]
pub fn mid_hook(args: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(Error::from(e).write_errors());
        }
    };

    let handler = match syn::parse::<ItemFn>(item) {
        Ok(handler) => handler,
        Err(err) => return err.to_compile_error().into(),
    };

    let args = match MacroArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.write_errors());
        }
    };

    if let Err(err) = validate_args(&args, &handler.sig) {
        return err.into();
    }

    // The handler shape selects the hook kind:
    //   fn(&mut Registers)                       -> resume   (JmpBack)
    //   fn(&mut Registers, original: usize) -> usize -> redirect (JmpToRet)
    // A redirect handler receives the original continuation address and returns
    // the address to jump to (return `original` to resume normally).
    let has_receiver = handler
        .sig
        .inputs
        .iter()
        .any(|arg| matches!(arg, syn::FnArg::Receiver(_)));
    let returns_unit = match &handler.sig.output {
        syn::ReturnType::Default => true,
        syn::ReturnType::Type(_, ty) => {
            matches!(&**ty, syn::Type::Tuple(tuple) if tuple.elems.is_empty())
        }
    };
    let redirect = match (has_receiver, handler.sig.inputs.len(), returns_unit) {
        (false, 1, true) => false,
        (false, 2, false) => true,
        _ => {
            return syn::Error::new_spanned(
                &handler.sig,
                "#[mid_hook] handler must be either `fn(&mut grappler::Registers)` (resume) or \
                 `fn(&mut grappler::Registers, original: usize) -> usize` (redirect)",
            )
            .to_compile_error()
            .into();
        }
    };

    let name = &handler.sig.ident;
    let fn_vis = &handler.vis;
    let mod_name = format_ident!("__{}", name);
    let struct_name = format_ident!("__{}Hook", name.to_string().to_table_case());
    let spanned_struct = quote! {
        #mod_name::#struct_name
    };
    let trampoline_name = format_ident!("__{}_trampoline", name);

    // Re-emit the user's handler under a private name so the trampoline can call
    // it; the trampoline is what ilhook invokes with the raw register pointer.
    let mut handler_fn = handler.clone();
    handler_fn.sig.ident = format_ident!("__{}_handler", name);
    let handler_name = &handler_fn.sig.ident;

    let address_fn = resolve_address_tokens(&args);
    let maybe_signature = option_tokens(args.signature.as_deref());
    let maybe_offset = option_tokens(args.offset);
    let trace_label = name.to_string();

    // Emit the trampoline matching the selected ilhook routine and pick the
    // corresponding HookType. A redirect trampoline forwards the original
    // continuation address and returns the handler's chosen jump target.
    // ilhook's routine ABI is architecture-specific (win64 on x86_64, cdecl on
    // x86). A proc-macro can't see the *target* arch, so emit a cfg-gated pair
    // of identical trampolines and let the build pick one.
    let (trampoline, hook_type) = if redirect {
        let body = quote! {
            grappler::core::trace!("Executing mid hook: {}", #trace_label);
            #handler_name(unsafe { &mut *regs }, ori_func_ptr)
        };
        (
            quote! {
                #[cfg(target_arch = "x86_64")]
                unsafe extern "win64" fn #trampoline_name(
                    regs: *mut Registers, ori_func_ptr: usize, _extra: usize,
                ) -> usize { #body }
                #[cfg(target_arch = "x86")]
                unsafe extern "cdecl" fn #trampoline_name(
                    regs: *mut Registers, ori_func_ptr: usize, _extra: usize,
                ) -> usize { #body }
            },
            quote! { grappler::core::ilhook_arch::HookType::JmpToRet(#trampoline_name) },
        )
    } else {
        let body = quote! {
            grappler::core::trace!("Executing mid hook: {}", #trace_label);
            #handler_name(unsafe { &mut *regs });
        };
        (
            quote! {
                #[cfg(target_arch = "x86_64")]
                unsafe extern "win64" fn #trampoline_name(regs: *mut Registers, _user_data: usize) { #body }
                #[cfg(target_arch = "x86")]
                unsafe extern "cdecl" fn #trampoline_name(regs: *mut Registers, _user_data: usize) { #body }
            },
            quote! { grappler::core::ilhook_arch::HookType::JmpBack(#trampoline_name) },
        )
    };

    let tokens = quote! {
        #handler_fn

        #[doc(hidden)]
        mod #mod_name {
            use std::str::FromStr;
            use grappler::core::poggers::structures::process::implement::utils::ProcessUtils as _;
            use grappler::core::Registers;
            use super::*;

            #trampoline

            pub struct #struct_name;

            impl #struct_name {
                pub fn initialize(&self) -> Result<(), Box<dyn std::error::Error>> {
                    let address: usize = unsafe {
                        #address_fn
                    };

                    let hooker = grappler::core::ilhook_arch::Hooker::new(
                        address,
                        #hook_type,
                        grappler::core::ilhook_arch::CallbackOption::None,
                        0,
                        grappler::core::ilhook_arch::HookFlags::empty(),
                    );

                    let hook_point = unsafe { hooker.hook()? };

                    // The hook stays installed for the life of the process;
                    // leak the HookPoint so its Drop (which would unhook) never
                    // runs.
                    std::mem::forget(hook_point);

                    Ok(())
                }

                pub fn signature(&self) -> Option<&str> {
                    #maybe_signature
                }

                pub fn offset(&self) -> Option<usize> {
                    #maybe_offset
                }
            }
        }

        #[allow(non_upper_case_globals)] #fn_vis const #name: #spanned_struct = #spanned_struct {};
    };

    TokenStream::from(tokens)
}
