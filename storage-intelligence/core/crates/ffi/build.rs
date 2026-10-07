// Generates C# P/Invoke bindings from the exported C ABI (ADR-001). If csbindgen output
// ever proves insufficient for a specific signature shape, fall back to hand-written
// bindings for that case (ADR-001's documented fallback) rather than fighting the tool.
fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");

    let out_dir = std::path::Path::new("../../../app/StorageIntelligence/Native");
    let _ = std::fs::create_dir_all(out_dir);

    let result = csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("ffi")
        .csharp_namespace("StorageIntelligence.Native")
        .csharp_class_name("NativeMethods")
        .generate_csharp_file(out_dir.join("NativeMethods.g.cs"));

    if let Err(err) = result {
        println!("cargo:warning=csbindgen generation failed, falling back to hand-written P/Invoke: {err}");
    }
}
