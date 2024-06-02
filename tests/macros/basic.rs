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

    assert_eq!(test::foo.signature(), "AB ?? CD");
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
