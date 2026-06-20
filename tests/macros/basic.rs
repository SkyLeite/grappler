#[test]
pub fn basic() {
    mod test {
        #[grappler::hook(signature = "AB BC CD DE")]
        pub fn foo() {}
    }

    assert_eq!(1, 1);
}

#[test]
pub fn can_call_original() {
    mod test {
        #[grappler::hook(signature = "AB BC CD DE")]
        pub fn foo() {
            foo.call_original();
        }
    }
}

#[test]
pub fn signature_matches() {
    mod test {
        #[grappler::hook(signature = "AB ?? CD")]
        pub fn foo() {}
    }

    assert_eq!(test::foo.signature(), Some("AB ?? CD"));
}

#[allow(dead_code)]
#[allow(unused_variables)]
#[test]
pub fn has_same_scope_as_original_fn() {
    mod test {
        mod some_module {
            pub const TEST: i32 = 1;
        }

        pub struct TestStruct;

        #[grappler::hook(signature = "AB CD")]
        pub fn foo(bar: TestStruct) {
            println!("{:#?}", some_module::TEST);
        }
    }

    assert_eq!(1, 1);
}

#[test]
pub fn can_use_extern_attribute() {
    mod test {
        #![allow(unsupported_calling_conventions)]
        #[grappler::hook(signature = "AB CD")]
        pub extern "fastcall" fn foo_fastcall() {}
    }
}

#[test]
pub fn can_supply_offset() {
    mod test {
        #[grappler::hook(offset = 0x10)]
        pub fn foo() {}
    }

    assert_eq!(1, 1)
}

#[test]
pub fn offset_matches() {
    mod test {
        #[grappler::hook(offset = 0x10)]
        pub fn foo() {}
    }

    assert_eq!(test::foo.offset(), Some(0x10));
}

#[test]
pub fn passthrough_signature_hook_has_no_body() {
    mod test {
        #[grappler::hook(signature = "AB CD")]
        pub fn foo(a: i32) -> i32;
    }

    assert_eq!(test::foo.signature(), Some("AB CD"));
}

#[test]
pub fn passthrough_offset_hook_has_no_body() {
    mod test {
        #[grappler::hook(offset = 0x10)]
        pub fn foo();
    }

    assert_eq!(test::foo.offset(), Some(0x10));
}

#[test]
pub fn mid_hook_by_offset() {
    mod test {
        #[grappler::mid_hook(offset = 0x20)]
        pub fn my_mid(regs: &mut grappler::Registers) {
            regs.rax = 1337;
        }
    }

    assert_eq!(test::my_mid.offset(), Some(0x20));
}

#[test]
pub fn mid_hook_by_signature() {
    mod test {
        #[grappler::mid_hook(signature = "AB CD")]
        pub fn my_mid(regs: &mut grappler::Registers) {
            let _ = regs;
        }
    }

    assert_eq!(test::my_mid.signature(), Some("AB CD"));
}
