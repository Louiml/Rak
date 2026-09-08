use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const KEYWORDS: &[&str] = &[
    "scan", "fetch", "dump", "trace", "loop", "if", "else", "fn", "let", "mut",
    "return", "use", "mod", "pub", "struct", "enum", "impl", "match", "for",
    "while", "break", "continue", "true", "false", "nil", "in",
    "try", "catch", "raise", "throw", "trait", "async", "await", "spawn", "as", "type",
];

const TYPES: &[&str] = &[
    "hex8", "hex16", "hex32", "hex64", "int", "string", "bytes", "bool",
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    "Option", "Result",
];

const BUILTINS: &[&str] = &[
    "fmt", "md5", "sha1", "sha256", "hex_encode", "hex_decode",
    "base64_encode", "base64_decode", "url_encode", "url_decode", "dns_lookup",
    "len", "split", "join", "contains", "to_hex", "from_hex", "int", "string",
    "float", "upper", "lower", "trim", "push", "keys", "values", "has", "get", "sort",
    "replace", "find", "starts_with", "ends_with", "slice", "repeat",
    "trim_start", "trim_end", "reverse", "min", "max", "sum", "abs", "sqrt", "pow", "clamp",
    "file_read", "file_write", "file_append", "file_exists", "file_size",
    "file_list", "file_delete", "file_mkdir", "file_copy", "file_rename",
    "file_ext", "file_basename", "file_dirname",
    "json_parse", "json_stringify", "json_get", "json_path", "json_keys",
    "line", "net_listen", "net_accept", "net_connect", "net_local_addr",
    "tcp_read", "tcp_write", "tcp_read_line", "tcp_close",
    "spawn", "thread_join", "channel", "chan_send", "chan_recv",
    "sleep", "now_ms", "args", "env_get", "env_set", "ord", "chr", "substr",
    "print", "dbg", "exit", "Some", "None", "Ok", "Err",
    "gui_open", "gui_update", "gui_title", "gui_close", "gui_wait", "gui_callback",
];

#[derive(Debug)]
struct Backend {
    client: Client,
    docs: std::sync::Mutex<std::collections::HashMap<Url, String>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let _ = self.client.log_message(MessageType::INFO, "Rak language server started").await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.docs.lock().unwrap().insert(uri.clone(), text.clone());
        self.publish_diagnostics(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let uri = params.text_document.uri;
            self.docs.lock().unwrap().insert(uri.clone(), change.text.clone());
            self.publish_diagnostics(&uri, &change.text).await;
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let docs = self.docs.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let pos = params.text_document_position.position;
        let offset = position_to_offset(source, pos);

        let cur = source.get(..offset).unwrap_or(source);
        let word = cur
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map(|i| &cur[i + 1..])
            .unwrap_or(cur);

        let mut items: Vec<CompletionItem> = Vec::new();
        for kw in KEYWORDS {
            if kw.starts_with(word) || word.is_empty() {
                items.push(completion_item(kw, CompletionItemKind::KEYWORD, "keyword"));
            }
        }
        for ty in TYPES {
            if ty.starts_with(word) || word.is_empty() {
                items.push(completion_item(ty, CompletionItemKind::CLASS, "type"));
            }
        }
        for b in BUILTINS {
            if b.starts_with(word) || word.is_empty() {
                items.push(completion_item(b, CompletionItemKind::FUNCTION, "builtin"));
            }
        }
        items.truncate(100);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let docs = self.docs.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let pos = params.text_document_position_params.position;
        let offset = position_to_offset(source, pos);
        let before = &source[..offset];
        let after = &source[offset..];
        let start = before.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map(|i| i + 1).unwrap_or(0);
        let end = offset + after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
        let word = &source[start..end];

        let details = if KEYWORDS.contains(&word) {
            format!("`{}` is a Rak keyword", word)
        } else if TYPES.contains(&word) {
            format!("`{}` is a builtin type", word)
        } else if BUILTINS.contains(&word) {
            format!("`{}` is a builtin function", word)
        } else {
            format!("`{}`", word)
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: details,
            }),
            range: None,
        }))
    }

    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let docs = self.docs.lock().unwrap();
        let Some(source) = docs.get(&uri) else {
            return Ok(None);
        };
        let pos = params.text_document_position_params.position;
        let offset = position_to_offset(source, pos);
        let before = &source[..offset];
        let after = &source[offset..];
        let start = before.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map(|i| i + 1).unwrap_or(0);
        let end = offset + after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
        let word = &source[start..end];

        if word.is_empty() {
            return Ok(None);
        }

        let patterns = [
            format!("fn {}(", word),
            format!("let {} ", word),
            format!("struct {} ", word),
            format!("struct {} {{", word),
            format!("enum {} ", word),
        ];

        for pat in &patterns {
            if let Some(idx) = source.find(pat) {
                let line = source[..idx].matches('\n').count() as u32;
                let col = (idx - source[..idx].rfind('\n').map(|i| i + 1).unwrap_or(0)) as u32;
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position { line, character: col },
                        end: Position { line, character: col + word.len() as u32 },
                    },
                })));
            }
        }

        Ok(None)
    }
}

impl Backend {
    async fn publish_diagnostics(&self, uri: &Url, source: &str) {
        let diags = match crate::lexer::tokenize(source) {
            Ok(tokens) => match crate::parser::parse(&tokens, source) {
                Ok(_) => Vec::new(),
                Err(e) => parse_error_to_diagnostic(source, &e.to_string()),
            },
            Err(e) => lex_error_to_diagnostic(source, &e.to_string()),
        };
        let _ = self.client.publish_diagnostics(uri.clone(), diags, None).await;
    }
}

fn completion_item(label: &str, kind: CompletionItemKind, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        ..Default::default()
    }
}

fn position_to_offset(source: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in source.lines().enumerate() {
        if i == pos.line as usize {
            return offset + (pos.character as usize).min(line.len());
        }
        offset += line.len() + 1;
    }
    offset
}

fn parse_error_to_diagnostic(source: &str, err: &str) -> Vec<Diagnostic> {
    let (line, col) = extract_line_col(source, err);
    vec![Diagnostic {
        range: Range {
            start: Position { line: line as u32, character: col as u32 },
            end: Position { line: line as u32, character: (col + 1) as u32 },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        message: err.to_string(),
        ..Default::default()
    }]
}

fn lex_error_to_diagnostic(source: &str, err: &str) -> Vec<Diagnostic> {
    let (line, col) = extract_line_col(source, err);
    vec![Diagnostic {
        range: Range {
            start: Position { line: line as u32, character: col as u32 },
            end: Position { line: line as u32, character: (col + 1) as u32 },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        message: err.to_string(),
        ..Default::default()
    }]
}

fn extract_line_col(source: &str, err: &str) -> (usize, usize) {
    if let Some(rest) = err.split("at line ").nth(1) {
        let parts: Vec<&str> = rest.split(", col ").collect();
        if parts.len() == 2 {
            let line = parts[0].trim().parse::<usize>().unwrap_or(1);
            let col = parts[1].trim().parse::<usize>().unwrap_or(1);
            return (line, col);
        }
    }
    let _ = source;
    (1, 1)
}

pub async fn start() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}