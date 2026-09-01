use scraper::{Html, Selector};
use std::collections::HashMap;

/// Parse an HTML string and extract the page title
pub fn html_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("title").ok()?;
    document.select(&selector).next().map(|e| e.text().collect::<String>().trim().to_string())
}

/// Select the first element matching a CSS selector and return its text content
pub fn html_select(html: &str, selector_str: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(selector_str).ok()?;
    document.select(&selector).next().map(|e| e.text().collect::<String>().trim().to_string())
}

/// Select all elements matching a CSS selector and return their text content
pub fn html_select_all(html: &str, selector_str: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    match Selector::parse(selector_str) {
        Ok(selector) => document.select(&selector).map(|e| e.text().collect::<String>().trim().to_string()).collect(),
        Err(_) => vec![],
    }
}

/// Extract an attribute from the first element matching a selector
pub fn html_attr(html: &str, selector_str: &str, attr: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(selector_str).ok()?;
    document.select(&selector).next().and_then(|e| e.value().attr(attr).map(|s| s.to_string()))
}

/// Extract all links (href attributes from <a> tags)
pub fn html_links(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    match Selector::parse("a[href]") {
        Ok(selector) => document.select(&selector).filter_map(|e| e.value().attr("href").map(|s| s.to_string())).collect(),
        Err(_) => vec![],
    }
}

/// Extract all image sources
pub fn html_images(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    match Selector::parse("img") {
        Ok(selector) => document.select(&selector).filter_map(|e| e.value().attr("src").map(|s| s.to_string())).collect(),
        Err(_) => vec![],
    }
}

/// Extract all scripts
pub fn html_scripts(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    match Selector::parse("script[src]") {
        Ok(selector) => document.select(&selector).filter_map(|e| e.value().attr("src").map(|s| s.to_string())).collect(),
        Err(_) => vec![],
    }
}

/// Extract all form action URLs
pub fn html_forms(html: &str) -> Vec<HashMap<String, String>> {
    let document = Html::parse_document(html);
    let mut forms = vec![];
    if let Ok(selector) = Selector::parse("form") {
        for form in document.select(&selector) {
            let mut form_info = HashMap::new();
            form_info.insert("action".to_string(), form.value().attr("action").unwrap_or("").to_string());
            form_info.insert("method".to_string(), form.value().attr("method").unwrap_or("GET").to_string());
            forms.push(form_info);
        }
    }
    forms
}

/// Extract meta tag content by name
pub fn html_meta(html: &str, name: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector_str = format!("meta[name=\"{}\"]", name);
    let selector = Selector::parse(&selector_str).ok()?;
    document.select(&selector).next().and_then(|e| e.value().attr("content").map(|s| s.to_string()))
}

/// Extract all input fields from forms
pub fn html_inputs(html: &str) -> Vec<HashMap<String, String>> {
    let document = Html::parse_document(html);
    let mut inputs = vec![];
    if let Ok(selector) = Selector::parse("input") {
        for input in document.select(&selector) {
            let mut input_info = HashMap::new();
            input_info.insert("name".to_string(), input.value().attr("name").unwrap_or("").to_string());
            input_info.insert("type".to_string(), input.value().attr("type").unwrap_or("text").to_string());
            input_info.insert("value".to_string(), input.value().attr("value").unwrap_or("").to_string());
            inputs.push(input_info);
        }
    }
    inputs
}

/// Count elements matching a selector
pub fn html_count(html: &str, selector_str: &str) -> usize {
    let document = Html::parse_document(html);
    match Selector::parse(selector_str) {
        Ok(selector) => document.select(&selector).count(),
        Err(_) => 0,
    }
}

/// Extract all headers (h1-h6)
pub fn html_headers(html: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let mut headers = vec![];
    if let Ok(selector) = Selector::parse("h1, h2, h3, h4, h5, h6") {
        for h in document.select(&selector) {
            headers.push(h.text().collect::<String>().trim().to_string());
        }
    }
    headers
}