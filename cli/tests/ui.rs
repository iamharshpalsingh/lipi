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

#[test]
fn hover_and_screen_size_styles_become_css_rules() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the style test");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out_dir = std::env::temp_dir().join(format!("lipi-styles-test-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["build", "tests/ui/styles.lipi", "--out", &out_dir.to_string_lossy()]).current_dir(root).output().unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new("node").arg(root.join("cli").join("tests").join("ui_dom.js")).arg(out_dir.join("app.js")).output().unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
    assert!(out.contains("class=\"lipi-button lipi-hover-"), "{out}");
    assert!(out.contains(":hover { background: #B83A22 !important }"), "{out}");
    assert!(out.contains("@media (max-width: 720px) { .lipi-mobile-") && out.contains("display: none !important"), "{out}");
    assert!(out.contains("@media (min-width: 721px) { .lipi-desktop-") && out.contains("gap: 24px !important"), "{out}");
}

#[test]
fn a_click_block_inside_a_for_acts_on_that_rows_item() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the loop-capture test");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out_dir = std::env::temp_dir().join(format!("lipi-loop-test-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["build", "tests/ui/loop_capture.lipi", "--out", &out_dir.to_string_lossy()]).current_dir(root).output().unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new("node")
        .arg(root.join("cli").join("tests").join("ui_dom.js"))
        .arg(out_dir.join("app.js"))
        .args(["click:pick chai", "click:pick lassi", "click:pick chai"])
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
    let last = out.rsplit("--- ").next().and_then(|frame| frame.split_once('\n')).map(|(_, html)| html).unwrap_or("");
    // One binding for the whole loop would put every click on "coffee".
    assert!(last.contains("Picked: chai"), "{out}");
    assert!(last.contains("chai — 2") && last.contains("lassi — 1") && last.contains("coffee — 0"), "{out}");
}

#[test]
fn keyed_components_keep_their_state_when_the_list_changes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["check", "tests/ui/keyed.lipi"]).current_dir(root).env("NO_COLOR", "1").output().unwrap();
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the rest of the keyed-component test");
        return;
    }
    let out_dir = std::env::temp_dir().join(format!("lipi-keyed-test-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["build", "tests/ui/keyed.lipi", "--out", &out_dir.to_string_lossy()]).current_dir(root).output().unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new("node")
        .arg(root.join("cli").join("tests").join("ui_dom.js"))
        .arg(out_dir.join("app.js"))
        .args(["click:+ Ravi", "click:+ Ravi", "click:remove Asha"])
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
    // The page after the last step (skipping the step's own label line).
    let last = out.rsplit("--- ").next().and_then(|frame| frame.split_once('\n')).map(|(_, html)| html).unwrap_or("");
    // Without keys, Ravi's two clicks would stay in the second row and land on Meera.
    assert!(last.contains("Ravi: 2") && last.contains("Meera: 0") && !last.contains("Asha"), "{out}");
}
