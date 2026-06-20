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
    macro_rules! bail {
        ($msg:expr) => {
            return syn::Error::new_spanned(&sig, $msg)
                .to_compile_error()
                .into()
        };
    }

    if sig
        .inputs
        .iter()
        .any(|arg| matches!(arg, syn::FnArg::Receiver(_)))
    {
        bail!("#[hook] cannot be applied to a function that takes `self`");
    }

    match (&args.signature, &args.offset) {
        (None, None) => bail!("#[hook] requires either a `signature` or an `offset` argument"),
        (Some(signature), _) => {
            if signature.is_empty() {
                bail!("Signature cannot be empty");
            }
            if signature.replace([' ', '?'], "").is_empty() {
                bail!("Signature must contain at least one known byte");
            }
        }
        (None, Some(_)) => {}
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

    // Argument validity (empty/known-byte signature, presence of a source) was
    // checked above, so the branches below are exhaustive.
    let address_fn = if let Some(ref signature) = args.signature {
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
            grappler::core::Signature::from_str(#signature)?.scan_module(module_name)?
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
        unreachable!("a missing signature and offset was rejected above");
    };

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
