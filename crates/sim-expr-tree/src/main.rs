#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let boot_args = sim_lib_expr_tree_serve::expr_tree_boot_args(std::env::args_os());
    match sim_lib_expr_tree_serve::expr_tree_bootloader().run(boot_args) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("sim-expr-tree: {err}");
            ExitCode::from(2)
        }
    }
}
