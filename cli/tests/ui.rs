//! LiPi UI: build examples/web_shop.lipi for the web and click through it in
//! a minimal DOM (tests/ui_dom.js) under Node.

use std::path::Path;
use std::process::Command;

#[test]
fn web_shop_draws_and_reacts_to_clicks() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the UI test");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out_dir = std::env::temp_dir().join(format!("lipi-ui-test-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(["build", "examples/web_shop.lipi", "--out", &out_dir.to_string_lossy()])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lipi");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let steps = ["click:Add to cart", "type:Search=lassi", "click:Add to cart", "type:Search=", "click:Checkout", "click:+", "click:+", "click:Back to the shop"];
    let run = Command::new("node")
        .arg(root.join("cli").join("tests").join("ui_dom.js"))
        .arg(out_dir.join("app.js"))
        .args(steps)
        .output()
        .expect("failed to run node");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
    assert!(run.status.success(), "{out}\n{}", String::from_utf8_lossy(&run.stderr));
    let frames: Vec<&str> = out.split("--- ").skip(1).collect();
    assert_eq!(frames.len(), steps.len() + 1, "{out}");
    let frame = |i: usize| frames[i];

    // First draw: three products, an empty cart.
    assert_eq!(frame(0).matches("Add to cart").count(), 3, "{}", frame(0));
    assert!(frame(0).contains("0 items · ₹0"), "{}", frame(0));
    // Adding a product updates its button and the cart total.
    assert!(frame(1).contains("<button class=\"lipi-button\" disabled type=\"button\">In the cart ✓</button>"), "{}", frame(1));
    assert!(frame(1).contains("1 items · ₹120"), "{}", frame(1));
    // Typing filters the list; the field keeps its value.
    assert!(frame(2).contains("[value=\"lassi\"]") && frame(2).contains("Mango Lassi") && !frame(2).contains("Masala Chai"), "{}", frame(2));
    assert!(frame(3).contains("2 items · ₹210"), "{}", frame(3));
    // Links change page; each Quantity component keeps its own state.
    assert!(frame(5).contains("<h1 class=\"lipi-heading\">Checkout</h1>"), "{}", frame(5));
    assert!(frame(7).contains("Masala Chai: 3") && frame(7).contains("Mango Lassi: 1"), "{}", frame(7));
    assert!(frame(8).contains("LiPi Shop") && frame(8).contains("2 items · ₹210"), "{}", frame(8));
    assert!(!out.contains("error on the page"), "{out}");
}

#[test]
fn ui_programs_explain_how_to_run_them() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["run", "examples/web_shop.lipi"]).current_dir(root).env("NO_COLOR", "1").output().unwrap();
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("LIP6002") && err.contains("lipi build"), "{err}");
    let node = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(["build", "examples/web_shop.lipi", "--target", "node", "--out", &std::env::temp_dir().join("lipi-ui-node").to_string_lossy()])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&node.stderr);
    assert!(err.contains("LIP3006") && err.contains("only works in web builds"), "{err}");
}
