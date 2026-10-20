//! HTML parsing builtins, in a build without the `scraper` crate.
//!
//! `scraper` needs `ahash`, which needs `getrandom`, whose browser backend
//! pulls in `wasm-bindgen` and a pile of generated glue. The browser build of
//! Rak has no use for it, so the dependency is left out and these functions
//! report that rather than pretending to have parsed something.
//!
//! The real implementation is `web.rs`; this file exists so the module keeps
//! the same shape either way.

use std::collections::HashMap;

fn no_html() -> String {
    "html: this build has no HTML parser (the `html` feature is off)".to_string()
}

/// Parse an HTML string and extract the page title
pub fn html_title(_html: &str) -> Option<String> {
    None
}

/// Extract the text of the first element matching a CSS selector.
pub fn html_select(_html: &str, _selector_str: &str) -> Option<String> {
    None
}

/// Extract the text of every element matching a CSS selector.
pub fn html_select_all(_html: &str, _selector_str: &str) -> Vec<String> {
    Vec::new()
}

/// Extract an attribute from the first element matching a CSS selector.
pub fn html_attr(_html: &str, _selector_str: &str, _attr: &str) -> Option<String> {
    None
}

/// Every `href` on the page.
pub fn html_links(_html: &str) -> Vec<String> {
    Vec::new()
}

/// Every `src` on the page.
pub fn html_images(_html: &str) -> Vec<String> {
    Vec::new()
}

/// Every script source on the page.
pub fn html_scripts(_html: &str) -> Vec<String> {
    Vec::new()
}

/// Every form on the page, as its fields.
pub fn html_forms(_html: &str) -> Vec<HashMap<String, String>> {
    Vec::new()
}

/// A `<meta>` tag's content.
pub fn html_meta(_html: &str, _name: &str) -> Option<String> {
    None
}

/// Every input on the page.
pub fn html_inputs(_html: &str) -> Vec<HashMap<String, String>> {
    Vec::new()
}

/// How many elements match a CSS selector.
pub fn html_count(_html: &str, _selector_str: &str) -> usize {
    0
}

/// Every response header in the page.
pub fn html_headers(_html: &str) -> Vec<String> {
    Vec::new()
}

/// The reason, for a caller that wants to report it.
pub fn html_unavailable() -> String {
    no_html()
}
