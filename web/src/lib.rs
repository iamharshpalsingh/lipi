//! The LiPi compiler as WebAssembly, for the browser playground.
//!
//! Build: `cargo build -p lipi_web --target wasm32-unknown-unknown --release`
//! (scripts/build-playground.ps1 does this and makes the playground page).
//!
//! The JavaScript side:
//! 1. `alloc(len)` gives a buffer; copy the UTF-8 source into it.
//! 2. `compile(ptr, len)` returns a pointer to a result: 4 bytes of length
//!    (little-endian), 1 byte of kind (0 = JavaScript, 1 = error text), then
//!    the text.
//! 3. `free(ptr, len)` releases a buffer or a result (result length = 5 + text length).

use lipi_compiler::codegen::{self, Target};

/// The interpreter's global names, so the playground checks programs exactly
/// like `lipi run` and `lipi build` do.
pub const BUILTINS: &[&str] = &[
    "toNumber", "toInteger", "toDecimal", "toString", "typeOf", "input", "assert", "assertEqual", "sleep", "all", "timeout",
    "math", "json", "fs", "env", "http", "time", "process", "server", "crypto", "database", "js", "regex", "encoding",
    "get", "post", "put", "patch", "delete",
    "page", "card", "row", "column", "section", "heading", "text", "button", "link", "image", "field", "checkbox", "element", "navigate",
];

/// Compile a program for the browser: JavaScript, or the error as text.
pub fn compile_text(source: &str) -> Result<String, String> {
    codegen::build_source("main.lipi", source, Target::Web, BUILTINS).map_err(|e| e.render(false))
}

fn leak(bytes: Vec<u8>) -> *mut u8 {
    Box::into_raw(bytes.into_boxed_slice()) as *mut u8
}

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    leak(vec![0u8; len])
}

/// # Safety
/// `ptr` and `len` must come from `alloc` or `compile` (for a result, len = 5 + text length).
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8, len: usize) {
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
}

/// # Safety
/// `ptr` must point to `len` readable bytes (from `alloc`).
#[no_mangle]
pub unsafe extern "C" fn compile(ptr: *const u8, len: usize) -> *mut u8 {
    let input = std::slice::from_raw_parts(ptr, len);
    let (kind, text) = match std::str::from_utf8(input) {
        Ok(source) => match compile_text(source) {
            Ok(js) => (0u8, js),
            Err(message) => (1u8, message),
        },
        Err(_) => (1u8, "The program isn't valid UTF-8 text.".to_string()),
    };
    let body = text.into_bytes();
    let mut out = Vec::with_capacity(5 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.push(kind);
    out.extend_from_slice(&body);
    leak(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_programs_and_reports_errors() {
        // Test threads get a small stack on Linux and macOS; unoptimised
        // compiler code needs more (the browser build is optimised).
        std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(check_programs).unwrap().join().unwrap();
    }

    fn check_programs() {
        let js = compile_text("show \"Namaste\"\n").unwrap();
        assert!(js.contains("$start(0)") && js.contains("Namaste"));
        let err = compile_text("naam = \"Asha\"\nshow nmae\n").unwrap_err();
        assert!(err.contains("LIP1002") && err.contains("did you mean \"naam\"?"), "{err}");
        // Server-only modules are refused in browser code.
        let err = compile_text("show fs.read(\"x\")\n").unwrap_err();
        assert!(err.contains("LIP6001"), "{err}");
    }
}
