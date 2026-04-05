use retarget::hook;

#[hook::observe(Sometimes)]
unsafe extern "C" fn invalid_mode(value: i32) -> i32 {
    value
}

fn main() {}
