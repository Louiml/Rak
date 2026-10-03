//! Format-spec rendering for `fmt(...)`, shared by both backends.
//!
//! This used to be two hand-written copies of the same `if spec.contains(...)`
//! chain -- one in `interpreter.rs`, one in `vm.rs` -- which had already drifted:
//! the interpreter had a float branch and the VM did not, so `fmt("{:.2f}", x)`
//! formatted on one backend and fell through to `to_string()` on the other. One
//! parser, one behaviour, plus a parity test that runs a table of specs through
//! both backends so they cannot drift again.
//!
//! The grammar is the useful subset of Rust's:
//!
//! ```text
//! [[fill]align][sign][#][0][width][.precision][type]
//! ```
//!
//! Supported types: `x` `X` `b` `o` `d` for integers, `f` `e` `E` for floats.
//! A spec with no type renders the value and applies only width and alignment.

/// A parsed format spec.
#[derive(Debug, Default, Clone, Copy)]
pub struct Spec {
    pub fill: Option<char>,
    pub align: Option<char>,
    pub sign: bool,
    pub alternate: bool,
    pub zero_pad: bool,
    pub width: Option<usize>,
    pub precision: Option<usize>,
    pub ty: Option<char>,
}

/// Parse a spec body. A single leading `:` is tolerated, because that is how the
/// callers see it: `"{:04X}"` hands over `":04X"`.
pub fn parse_spec(body: &str) -> Spec {
    let s = body.strip_prefix(':').unwrap_or(body);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut spec = Spec::default();

    // `[[fill]align]` -- an align character may be preceded by a fill character.
    if chars.len() >= 2 && is_align(chars[1]) && !is_type(chars[0]) {
        spec.fill = Some(chars[0]);
        spec.align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        spec.align = Some(chars[0]);
        i = 1;
    }
    if i < chars.len() && (chars[i] == '+' || chars[i] == ' ') {
        // Only `+` is tracked. A bare space is accepted and ignored, because
        // rejecting a spec outright is a worse failure than ignoring one flag.
        spec.sign = chars[i] == '+';
        i += 1;
    }
    if i < chars.len() && chars[i] == '#' {
        spec.alternate = true;
        i += 1;
    }
    // A `0` before the width means "pad with zeros", unless a fill was given.
    if i < chars.len() && chars[i] == '0' && spec.fill.is_none() {
        spec.zero_pad = true;
        i += 1;
    }
    let width_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > width_start {
        spec.width = chars[width_start..i]
            .iter()
            .collect::<String>()
            .parse()
            .ok();
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        let p_start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        spec.precision = Some(
            chars[p_start..i]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(0),
        );
    }
    if i < chars.len() && is_type(chars[i]) {
        spec.ty = Some(chars[i]);
    }
    spec
}

fn is_align(c: char) -> bool {
    c == '<' || c == '>' || c == '^'
}

fn is_type(c: char) -> bool {
    matches!(c, 'x' | 'X' | 'b' | 'o' | 'd' | 'f' | 'e' | 'E' | 's')
}

/// Render the integer body for a spec's type, without padding.
fn render_int(v: u64, spec: &Spec) -> String {
    let ty = spec.ty.unwrap_or('d');
    let digits = match ty {
        'X' => format!("{:X}", v),
        'x' => format!("{:x}", v),
        'b' => format!("{:b}", v),
        'o' => format!("{:o}", v),
        // `f`/`e` on an integer value degrades to decimal rather than refusing.
        _ => format!("{}", v),
    };
    if spec.alternate {
        match ty {
            'x' | 'X' => return format!("0x{}", digits),
            'b' => return format!("0b{}", digits),
            'o' => return format!("0o{}", digits),
            _ => {}
        }
    }
    digits
}

/// Render the float body for a spec's type, without padding.
fn render_float(v: f64, spec: &Spec) -> String {
    match spec.ty {
        Some('e') => format!("{:.*e}", spec.precision.unwrap_or(6), v),
        Some('E') => format!("{:.*E}", spec.precision.unwrap_or(6), v),
        Some('f') => format!("{:.*}", spec.precision.unwrap_or(6), v),
        // A float with a hex/octal/binary type goes through the integer path:
        // that is what a hex dump of a float field wants.
        _ => render_int(v as u64, spec),
    }
}

/// Render one field. `int` and `float` are the value's numeric readings and
/// `display` its default rendering; a caller supplies whichever it has.
pub fn render(spec_body: &str, int: Option<u64>, float: Option<f64>, display: &str) -> String {
    let spec = parse_spec(spec_body);
    let numeric = int.is_some() || float.is_some();

    // The sign lives outside the digits so that padding cannot eat it.
    let (sign, magnitude) = match display.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", display),
    };

    // A negative value's `as_u64` is the two's-complement bit pattern, so `-42`
    // arrives as `2^64 - 42` and rendered as an unsigned magnitude. Flip it back.
    let int = match (sign, int) {
        ("-", Some(v)) => Some(v.wrapping_neg()),
        (_, v) => v,
    };

    let digits = match (spec.ty, int, float) {
        (Some('f') | Some('e') | Some('E'), _, Some(f)) => render_float(f, &spec),
        (_, Some(v), _) => render_int(v, &spec),
        (_, None, Some(f)) => render_float(f, &spec),
        _ => magnitude.to_string(),
    };
    let signed = if !sign.is_empty() {
        format!("{}{}", sign, digits)
    } else if spec.sign && numeric {
        format!("+{}", digits)
    } else {
        digits
    };

    // Zero-padding: pad after the sign, and ignore alignment, because `0` is
    // shorthand for `0>width`. The width counts the sign, matching Rust, Python
    // and Go, so `{:05}` of -42 is `-0042` rather than `-00042`.
    let zero_pad = spec.zero_pad || (spec.fill == Some('0') && spec.align.is_none());
    if zero_pad {
        if let Some(width) = spec.width {
            let sign_len = usize::from(signed.starts_with('-') || signed.starts_with('+'));
            let body_len = signed.chars().count() - sign_len;
            if sign_len + body_len < width {
                let (head, tail) = signed.split_at(sign_len);
                let zeros = "0".repeat(width - sign_len - body_len);
                return format!("{}{}{}", head, zeros, tail);
            }
        }
        return signed;
    }

    // Width and alignment. Without an explicit alignment, numbers go right and
    // everything else goes left.
    let Some(width) = spec.width else {
        return signed;
    };
    let len = signed.chars().count();
    if len >= width {
        return signed;
    }
    let missing = width - len;
    let fill = spec.fill.unwrap_or(' ');
    match spec.align.unwrap_or(if numeric { '>' } else { '<' }) {
        '<' => format!("{}{}", signed, fill.to_string().repeat(missing)),
        '^' => {
            let left = missing / 2;
            format!(
                "{}{}{}",
                fill.to_string().repeat(left),
                signed,
                fill.to_string().repeat(missing - left)
            )
        }
        _ => format!("{}{}", fill.to_string().repeat(missing), signed),
    }
}
