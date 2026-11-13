//! Console front end: export, simulate, render, section, random.
//! Without arguments it prints the help.

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        args.push("--help".into());
    }
    match lampdesigner::cli::run(&args) {
        Some(code) => std::process::exit(code),
        None => {
            eprintln!("unknown command '{}'; try --help", args[1]);
            std::process::exit(2);
        }
    }
}
