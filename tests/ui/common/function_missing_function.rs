use retarget::hook;

#[hook::function]
unsafe extern "C" fn missing_target(value: i32) -> i32 {
    value
}

fn main() {}
