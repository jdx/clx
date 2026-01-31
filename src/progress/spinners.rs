//! Spinner definitions for animated progress indicators.

use std::collections::HashMap;
use std::sync::LazyLock;

/// A spinner animation definition.
pub(crate) struct Spinner {
    pub frames: Vec<String>,
    pub fps: usize,
}

macro_rules! spinner {
    ($name:expr, $frames:expr, $fps:expr) => {
        (
            $name.to_string(),
            Spinner {
                frames: $frames.iter().map(|s| s.to_string()).collect(),
                fps: $fps,
            },
        )
    };
}

/// Default spinner name.
pub(crate) const DEFAULT_SPINNER: &str = "mini_dot";

/// Default body template for progress jobs.
pub(crate) static DEFAULT_BODY: LazyLock<String> =
    LazyLock::new(|| "{{ spinner() }} {{ message }}".to_string());

/// Collection of available spinner animations.
#[rustfmt::skip]
pub(crate) static SPINNERS: LazyLock<HashMap<String, Spinner>> = LazyLock::new(|| {
    vec![
        // Classic - from https://github.com/charmbracelet/bubbles/blob/ea344ab907bddf5e8f71cd73b9583b070e8f1b2f/spinner/spinner.go
        spinner!("line", &["|", "/", "-", "\\"], 200),
        spinner!("dot", &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"], 200),
        spinner!("mini_dot", &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"], 200),
        spinner!("jump", &["⢄", "⢂", "⢁", "⡁", "⡈", "⡐", "⡠"], 200),
        spinner!("pulse", &["█", "▓", "▒", "░"], 200),
        spinner!("points", &["∙∙∙", "●∙∙", "∙●∙", "∙∙●"], 200),
        spinner!("globe", &["🌍", "🌎", "🌏"], 400),
        spinner!("moon", &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"], 400),
        spinner!("monkey", &["🙈", "🙉", "🙊"], 400),
        spinner!("meter", &["▱▱▱", "▰▱▱", "▰▰▱", "▰▰▰", "▰▰▱", "▰▱▱", "▱▱▱"], 400),
        spinner!("hamburger", &["☱", "☲", "☴", "☲"], 200),
        spinner!("ellipsis", &["   ", ".  ", ".. ", "..."], 200),
        // Classic/Minimal
        spinner!("arrow", &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"], 200),
        spinner!("triangle", &["◢", "◣", "◤", "◥"], 200),
        spinner!("square", &["◰", "◳", "◲", "◱"], 200),
        spinner!("circle", &["◴", "◷", "◶", "◵"], 200),
        // Box Drawing
        spinner!("bounce", &["⠁", "⠂", "⠄", "⠂"], 200),
        spinner!("arc", &["◜", "◠", "◝", "◞", "◡", "◟"], 200),
        spinner!("box_bounce", &["▖", "▘", "▝", "▗"], 200),
        // Aesthetic
        spinner!("star", &["✶", "✸", "✹", "✺", "✹", "✷"], 200),
        spinner!("hearts", &["💛", "💙", "💜", "💚", "❤️"], 400),
        spinner!("clock", &["🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛"], 200),
        spinner!("weather", &["🌤", "⛅", "🌥", "☁️", "🌧", "⛈", "🌩", "🌨"], 400),
        // Growing/Progress-like
        spinner!("grow_horizontal", &["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█", "▉", "▊", "▋", "▌", "▍", "▎"], 200),
        spinner!("grow_vertical", &["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂"], 200),
        // Playful
        spinner!("runner", &["🚶", "🏃"], 400),
        spinner!("oranges", &["🍊", "🍋", "🍇", "🍎"], 400),
        spinner!("smiley", &["😀", "😬", "😁", "😂", "🤣", "😂", "😁", "😬"], 400),
    ]
    .into_iter()
    .collect()
});
