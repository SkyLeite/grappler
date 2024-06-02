#[cfg(test)]
extern crate grappler;

mod macros;

#[test]
pub fn test_macros() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/macros/*-fail.rs");
}
