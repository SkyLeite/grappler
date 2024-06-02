#[test]
pub fn basic() {
    #[grappler::hook(signature = "AB BC CD DE")]
    pub fn foo() {}

    assert_eq!(1, 1);
}

#[test]
pub fn signature_matches() {
    #[grappler::hook(signature = "AB ?? CD")]
    pub fn foo() {}

    assert_eq!(foo.signature(), "AB ?? CD");
}
