use console::{StyledObject, style};

pub fn ereset() -> String {
    if console::colors_enabled_stderr() {
        "\x1b[0m".to_string()
    } else {
        "".to_string()
    }
}

pub fn estyle<D>(val: D) -> StyledObject<D> {
    style(val).for_stderr()
}

pub fn ecyan<D>(val: D) -> StyledObject<D> {
    estyle(val).cyan()
}

pub fn eblue<D>(val: D) -> StyledObject<D> {
    estyle(val).blue()
}

pub fn emagenta<D>(val: D) -> StyledObject<D> {
    estyle(val).magenta()
}

pub fn egreen<D>(val: D) -> StyledObject<D> {
    estyle(val).green()
}

pub fn eyellow<D>(val: D) -> StyledObject<D> {
    estyle(val).yellow()
}

pub fn ered<D>(val: D) -> StyledObject<D> {
    estyle(val).red()
}

pub fn eblack<D>(val: D) -> StyledObject<D> {
    estyle(val).black()
}

pub fn eunderline<D>(val: D) -> StyledObject<D> {
    estyle(val).underlined()
}

pub fn edim<D>(val: D) -> StyledObject<D> {
    estyle(val).dim()
}

pub fn ebold<D>(val: D) -> StyledObject<D> {
    estyle(val).bold()
}

pub fn nstyle<D>(val: D) -> StyledObject<D> {
    style(val).for_stdout()
}

pub fn ncyan<D>(val: D) -> StyledObject<D> {
    nstyle(val).cyan()
}

pub fn nunderline<D>(val: D) -> StyledObject<D> {
    nstyle(val).underlined()
}

pub fn nyellow<D>(val: D) -> StyledObject<D> {
    nstyle(val).yellow()
}

pub fn nred<D>(val: D) -> StyledObject<D> {
    nstyle(val).red()
}

pub fn ndim<D>(val: D) -> StyledObject<D> {
    nstyle(val).dim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ereset() {
        let reset = ereset();
        // ereset returns either the ANSI reset code or empty string
        // depending on whether colors are enabled
        assert!(reset.is_empty() || reset == "\x1b[0m");
    }

    #[test]
    fn test_estyle_returns_styled_object() {
        let styled = estyle("test");
        let output = styled.to_string();
        assert!(output.contains("test"));
    }

    #[test]
    fn test_stderr_color_functions() {
        // All e-prefixed functions should return StyledObjects containing the input
        assert!(ecyan("test").to_string().contains("test"));
        assert!(eblue("test").to_string().contains("test"));
        assert!(emagenta("test").to_string().contains("test"));
        assert!(egreen("test").to_string().contains("test"));
        assert!(eyellow("test").to_string().contains("test"));
        assert!(ered("test").to_string().contains("test"));
        assert!(eblack("test").to_string().contains("test"));
    }

    #[test]
    fn test_stderr_format_functions() {
        assert!(eunderline("test").to_string().contains("test"));
        assert!(edim("test").to_string().contains("test"));
        assert!(ebold("test").to_string().contains("test"));
    }

    #[test]
    fn test_nstyle_returns_styled_object() {
        let styled = nstyle("test");
        let output = styled.to_string();
        assert!(output.contains("test"));
    }

    #[test]
    fn test_stdout_color_functions() {
        assert!(ncyan("test").to_string().contains("test"));
        assert!(nyellow("test").to_string().contains("test"));
        assert!(nred("test").to_string().contains("test"));
    }

    #[test]
    fn test_stdout_format_functions() {
        assert!(nunderline("test").to_string().contains("test"));
        assert!(ndim("test").to_string().contains("test"));
    }

    #[test]
    fn test_style_chaining() {
        // Test that style functions can be chained
        let styled = ecyan("test").bold();
        assert!(styled.to_string().contains("test"));

        let styled = ered("test").underlined().bold();
        assert!(styled.to_string().contains("test"));
    }

    #[test]
    fn test_style_with_different_types() {
        // Test with different input types
        assert!(ecyan(42).to_string().contains("42"));
        assert!(eblue(3.14).to_string().contains("3.14"));
        assert!(egreen(true).to_string().contains("true"));
    }

    #[test]
    fn test_style_with_empty_string() {
        let styled = ecyan("");
        let output = styled.to_string();
        // Output should be empty or contain only ANSI codes
        assert!(output.is_empty() || !output.chars().any(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_style_with_special_characters() {
        let styled = ecyan("hello\nworld");
        assert!(styled.to_string().contains("hello"));
        assert!(styled.to_string().contains("world"));

        let styled = eyellow("tab\there");
        assert!(styled.to_string().contains("tab"));
        assert!(styled.to_string().contains("here"));
    }
}
