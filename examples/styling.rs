//! Example demonstrating clx styling functions
//!
//! Run with: cargo run --example styling

use clx::style;

fn main() {
    println!("=== clx Styling Demo ===\n");

    // Stderr styling (e-prefixed functions)
    eprintln!("--- Stderr Styling (e-prefixed) ---");
    eprintln!("  {}  - ecyan", style::ecyan("cyan text"));
    eprintln!("  {}  - eblue", style::eblue("blue text"));
    eprintln!("  {}  - emagenta", style::emagenta("magenta text"));
    eprintln!("  {}  - egreen", style::egreen("green text"));
    eprintln!("  {}  - eyellow", style::eyellow("yellow text"));
    eprintln!("  {}  - ered", style::ered("red text"));
    eprintln!("  {}  - eblack", style::eblack("black text"));
    eprintln!("  {}  - eunderline", style::eunderline("underlined"));
    eprintln!("  {}  - edim", style::edim("dimmed text"));
    eprintln!("  {}  - ebold", style::ebold("bold text"));
    eprintln!();

    // Stdout styling (n-prefixed functions)
    println!("--- Stdout Styling (n-prefixed) ---");
    println!("  {}  - ncyan", style::ncyan("cyan text"));
    println!("  {}  - nyellow", style::nyellow("yellow text"));
    println!("  {}  - nred", style::nred("red text"));
    println!("  {}  - nunderline", style::nunderline("underlined"));
    println!("  {}  - ndim", style::ndim("dimmed text"));
    println!();

    // Combining styles
    println!("--- Combined Styles ---");
    eprintln!(
        "  {}  - bold + cyan",
        style::ecyan("bold cyan").bold()
    );
    eprintln!(
        "  {}  - dim + underline",
        style::edim("dim underlined").underlined()
    );
    eprintln!(
        "  {}  - bright + green",
        style::egreen("bright green").bright()
    );
    println!();

    // Reset sequence
    println!("--- Reset Sequence ---");
    let reset = style::ereset();
    if reset.is_empty() {
        println!("  (colors disabled, reset is empty string)");
    } else {
        println!("  Reset sequence available: {:?}", reset.as_bytes());
    }
    println!();

    // Using estyle for custom styling
    println!("--- Custom Styling with estyle/nstyle ---");
    eprintln!(
        "  {}",
        style::estyle("custom: bold + italic + red")
            .bold()
            .italic()
            .red()
    );
    println!(
        "  {}",
        style::nstyle("custom: blink + magenta")
            .blink()
            .magenta()
    );
}
