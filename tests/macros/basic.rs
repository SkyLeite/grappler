#[test]
pub fn basic() {
    mod test {
        #[grappler::hook(signature = "AB BC CD DE")]
        pub fn foo() {}
    }

    assert_eq!(1, 1);
}

#[test]
pub fn signature_matches() {
    mod test {
        #[grappler::hook(signature = "AB ?? CD")]
        pub fn foo() {}
    }

    assert_eq!(test::foo.signature(), "AB ?? CD");
}

#[test]
pub fn has_same_scope_as_original_fn() {
    mod test {
        mod some_module {
            pub const TEST: i32 = 1;
        }

        #[grappler::hook(signature = "AB CD")]
        pub fn foo() {
            println!("{:#?}", some_module::TEST);
        }
    }

    assert_eq!(1, 1);
}
