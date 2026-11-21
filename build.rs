fn main() {
    // One fixed widget style, so the window looks the same on every platform.
    #[cfg(feature = "gui")]
    slint_build::compile_with_config(
        "src/gui/ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .expect("the Slint UI failed to compile");

    // Cargo otherwise runs this again, and builds the crate again, whenever
    // any file of the package changes.
    #[cfg(not(feature = "gui"))]
    println!("cargo:rerun-if-changed=build.rs");
}
