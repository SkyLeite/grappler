extern crate inflector;

use darling::{ast::NestedMeta, Error, FromMeta};
use inflector::Inflector;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn, Pat};

#[derive(Debug, FromMeta)]
struct MacroArgs {
    signature: String,
}

#[proc_macro_attribute]
pub fn hook(args: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(Error::from(e).write_errors());
        }
    };

    let input = parse_macro_input!(item as ItemFn);

    let _args = match MacroArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.write_errors());
        }
    };

    let signature = _args.signature;

    if signature.len() < 1 {
        panic!("Signature cannot be empty");
    }

    if signature.replace(" ", "").replace("?", "").len() < 1 {
        panic!("Signature must contain at least one known byte");
    }

    let name = &input.sig.ident;
    let fn_vis = &input.vis;
    let inputs = &input.sig.inputs;
    let output = &input.sig.output;
    let mod_name = format_ident!("__{}", name);
    let struct_name = format_ident!("__{}Hook", name.to_string().to_table_case());
    let spanned_struct = quote! {
        #mod_name::#struct_name
    };

    let retour_fn_name = format_ident!("__{}Retour", name);
    let retour_fn_abi = &input.sig.abi;

    let mut new_fn = input.clone();
    new_fn.sig.ident = format_ident!("__{}_original", name);

    let new_fn_name = &new_fn.sig.ident;
    let new_fn_name_str = &new_fn.sig.ident.to_string();

    let input_types: Vec<_> = inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => &*pat_type.ty,
            _ => panic!("Unexpected argument type"),
        })
        .collect();

    let input_names: Vec<_> = inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    Some(&pat_ident.ident)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    let fn_sig = quote! {
        fn(#(#input_types),*) #output
    };

    let tokens = quote! {
        #new_fn

        #[doc(hidden)]
        mod #mod_name {
            use std::str::FromStr;
            use super::*;

            grappler::core::static_detour! {
                pub static #retour_fn_name: #retour_fn_abi #fn_sig;
            }

            pub struct #struct_name;

            impl #struct_name {
                pub fn initialize(&self) {
                    let address = unsafe {
                        grappler::core::Signature::from_str(#signature)
                            .map(|sig| sig.scan_module(std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap()))
                            .unwrap()
                            .unwrap()
                    };

                    let pointer = unsafe { std::mem::transmute(address) };

                    unsafe {
                        #retour_fn_name.initialize(pointer, |#(#input_names),*| {
                            grappler::core::trace!("Executing hook: {}", #new_fn_name_str);
                            #new_fn_name(#(#input_names),*)
                        }).unwrap().enable().unwrap();
                    }
                }

                pub fn initialize_ptr(&self, ptr: *mut u8) {
                    let pointer = unsafe { std::mem::transmute(ptr) };

                    unsafe {
                        #retour_fn_name.initialize(pointer, |#(#input_names),*| {
                            grappler::core::trace!("Executing hook: {}", #new_fn_name_str);
                            #new_fn_name(#(#input_names),*)
                        }).unwrap().enable().unwrap();
                    }
                }

                pub fn call_original(&self, #inputs) #output {
                    #retour_fn_name.call(#(#input_names),*)
                }

                pub fn signature(&self) -> &str {
                    #signature
                }
            }
        }

        #[allow(non_upper_case_globals)] #fn_vis const #name: #spanned_struct = #spanned_struct {};
    };

    TokenStream::from(tokens)
}
