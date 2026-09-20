use std::io::{self, BufReader};

use keepassxc_control_helper::Helper;

fn main() {
    let mut helper = Helper::new();
    helper.run(BufReader::new(io::stdin()), io::stdout());
}
