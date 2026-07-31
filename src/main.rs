use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("fmt") => cmd_fmt(&args[1..]),
        Some(other) => {
            eprintln!("foxy: unknown command '{other}'\n");
            print_usage();
            ExitCode::FAILURE
        }
        None => {
            print_usage();
            ExitCode::SUCCESS
        }
    }
}

fn cmd_fmt(paths: &[String]) -> ExitCode {
    if paths.is_empty() {
        println!("foxy fmt: not yet implemented (would format all .foxy files in the project)");
    } else {
        for path in paths {
            println!("foxy fmt: not yet implemented (would format {path})");
        }
    }
    ExitCode::SUCCESS
}

fn print_usage() {
    println!(
        "foxy — the Foxy language toolchain\n\
         \n\
         USAGE:\n\
         \x20   foxy <command> [args]\n\
         \n\
         COMMANDS:\n\
         \x20   fmt [path...]    Format Foxy source files (placeholder — not yet implemented)"
    );
}
