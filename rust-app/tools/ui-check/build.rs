fn main() {
    slint_build::compile_with_config(
        "../../crates/worker/ui/main.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .expect("Slint UI must compile before native dependencies are built");
}
