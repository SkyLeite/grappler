mod test {
    #[grappler::hook(signature = "AB CD")]
    fn foo() {}
}

pub fn main() {
    let _bar = test::foo;
}
