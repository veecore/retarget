use retarget::hook;

#[hook::function(optional = true, "puts")]
unsafe extern "C" fn positional_target_after_named_arg(value: i32) -> i32 {
    value
}

fn main() {}
