#![windows_subsystem = "windows"]

fn main() {
    let mut arguments = std::env::args_os().skip(1).peekable();
    if arguments
        .peek()
        .is_some_and(|argument| argument == "--worker")
    {
        arguments.next();
        if worker::run(arguments).is_err() {
            std::process::exit(1);
        }
    } else if let Err(error) = tray_host::run(arguments) {
        tray_host::show_error(&error.to_string());
    }
}
