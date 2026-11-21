fn main() {
    // Material 3 (dark) for the few standard widgets used (switches, lists).
    let config = slint_build::CompilerConfiguration::new().with_style("material-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compile the interface");
}
