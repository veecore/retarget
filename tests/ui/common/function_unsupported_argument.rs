use retarget::hook;

#[hook::function(symbol = "puts")]
unsafe extern "C" fn unsupported_argument(value: i32) -> i32 {
    value
}

fn main() {}
