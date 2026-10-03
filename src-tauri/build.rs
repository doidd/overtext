fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Tauri links its Windows manifest to application binaries only. Native
        // Windows controls also need Common Controls v6 in unit-test executables.
        let resource = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("resource.rc");
        embed_resource::compile_for_tests(resource, embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
