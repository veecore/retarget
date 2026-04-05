use retarget::hook;

#[hook::observe(value = 1usize, mode = Sometimes)]
unsafe extern "C" fn invalid_mode(value: i32) -> i32 {
    value
}

fn main() {}
