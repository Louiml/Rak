use crate::ast::*;
use crate::lexer::{tokenize, Token};
use crate::RakError;
use crate::Result;

fn map_int_kind(k: crate::lexer::IntKind) -> IntKind {
    use crate::lexer::IntKind as L;
    match k {
        L::I8 => IntKind::I8,
        L::I16 => IntKind::I16,
        L::I32 => IntKind::I32,
        L::I64 => IntKind::I64,
        L::U8 => IntKind::U8,
        L::U16 => IntKind::U16,
        L::U32 => IntKind::U32,
        L::U64 => IntKind::U64,
    }
}

pub fn parse(tokens: &[(Token, usize)], source: &str) -> Result<Module> {
    let mut parser = Parser::new(tokens, source);
    parser.parse_module()
}

pub fn parse_expr_str(source: &str) -> Result<Expr> {
    let tokens = tokenize(source)?;
    let mut p = Parser::new(&tokens, source);
    p.parse_expr()
}

struct Parser<'a> {
    tokens: &'a [(Token, usize)],
    source: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [(Token, usize)], source: &'a str) -> Self {
        Parser { tokens, source, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n).map(|(t, _)| t)
    }

    fn cur_off(&self) -> usize {
        self.tokens.get(self.pos).map(|(_, o)| *o).unwrap_or(self.source.len())
    }

    fn span_here(&self) -> (usize, usize) {
        crate::lexer::offset_to_line_col(self.source, self.cur_off())
    }

    fn perr(&self, msg: String) -> RakError {
        let (line, col) = self.span_here();
        RakError::Parser(format!("{} at line {}, col {}", msg, line, col))
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos).map(|(t, _)| t);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<&Token> {
        match self.peek() {
            Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(&expected) => {
                Ok(self.advance().unwrap())
            }
            Some(tok) => Err(self.perr(format!("Expected {:?}, found {:?}", expected, tok))),
            None => Err(self.perr(format!("Expected {:?}, found end of file", expected))),
        }
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        match self.peek() {
            Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn check(&self, expected: &Token) -> bool {
        matches!(self.peek(), Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected))
    }

    fn semi(&mut self) -> Result<()> {
        let _ = self.match_token(&Token::Semi);
        Ok(())
    }

    fn parse_module(&mut self) -> Result<Module> {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        while self.peek().is_some() {
            if self.check(&Token::Use) {
                self.advance();
                imports.push(self.parse_import(false)?);
                self.semi()?;
            } else if self.check(&Token::Import) {
                self.advance();
                imports.push(self.parse_import(false)?);
                self.semi()?;
            } else if self.check(&Token::From) {
                imports.push(self.parse_from_import(false)?);
                self.semi()?;
            } else if (self.check(&Token::Pub) || self.check(&Token::Export))
                && matches!(self.peek_n(1), Some(Token::Use))
            {
                // `pub use m` / `export use m` / `pub use {a} from m` re-export.
                self.advance(); // pub/export
                self.advance(); // use
                imports.push(self.parse_reexport()?);
                self.semi()?;
            } else if (self.check(&Token::Pub) || self.check(&Token::Export))
                && matches!(self.peek_n(1), Some(Token::Import))
            {
                // `pub import m` / `export import m` — re-export a whole module.
                self.advance(); // pub/export
                self.advance(); // import
                let mut imp = self.parse_import(false)?;
                imp.reexport = true;
                imports.push(imp);
                self.semi()?;
            } else if (self.check(&Token::Pub) || self.check(&Token::Export))
                && matches!(self.peek_n(1), Some(Token::From))
            {
                // `pub from m import ...` — re-export names.
                self.advance(); // pub/export
                let mut imp = self.parse_from_import(false)?;
                imp.reexport = true;
                imports.push(imp);
                self.semi()?;
            } else {
                items.push(self.parse_stmt()?);
            }
        }
        Ok(Module { imports, items })
    }

    fn parse_import(&mut self, reexport: bool) -> Result<Import> {
        if let Some(Token::String(s)) = self.peek() {
            let s = s.clone();
            self.advance();
            let alias = if self.match_token(&Token::As) {
                if let Some(Token::Ident(a)) = self.peek() {
                    let a = a.clone();
                    self.advance();
                    Some(a)
                } else {
                    None
                }
            } else {
                None
            };
            return Ok(Import {
                path: vec![s],
                is_file: true,
                alias,
                kind: ImportKind::Whole,
                from_names: vec![],
                star: false,
                reexport,
            });
        }

        let mut path = vec![];
        if let Some(Token::Ident(name)) = self.peek() {
            path.push(name.clone());
            self.advance();
        } else {
            return Err(self.perr("Expected import path".to_string()));
        }
        while self.match_token(&Token::Dot) || self.match_token(&Token::ColonColon) {
            if let Some(Token::Ident(name)) = self.peek() {
                path.push(name.clone());
                self.advance();
            } else {
                return Err(self.perr("Expected identifier after path separator".to_string()));
            }
        }
        let alias = if self.match_token(&Token::As) {
            if let Some(Token::Ident(a)) = self.peek() {
                let a = a.clone();
                self.advance();
                Some(a)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Import {
            path,
            is_file: false,
            alias,
            kind: ImportKind::Whole,
            from_names: vec![],
            star: false,
            reexport,
        })
    }

    /// Parse `from m import x, y as z` / `from m import *`.
    fn parse_from_import(&mut self, reexport: bool) -> Result<Import> {
        self.expect(Token::From)?;
        // The module target: a file path string or a dotted name.
        let (path, is_file) = if let Some(Token::String(s)) = self.peek() {
            let s = s.clone();
            self.advance();
            (vec![s], true)
        } else {
            let mut path = vec![];
            if let Some(Token::Ident(name)) = self.peek() {
                path.push(name.clone());
                self.advance();
            } else {
                return Err(self.perr("Expected module name after 'from'".to_string()));
            }
            while self.match_token(&Token::Dot) || self.match_token(&Token::ColonColon) {
                if let Some(Token::Ident(name)) = self.peek() {
                    path.push(name.clone());
                    self.advance();
                } else {
                    return Err(self.perr("Expected identifier after path separator".to_string()));
                }
            }
            (path, false)
        };
        self.expect(Token::Import)?;
        // `*` or a comma-separated name list with optional `as alias`.
        if self.match_token(&Token::Star) {
            return Ok(Import {
                path,
                is_file,
                alias: None,
                kind: ImportKind::From,
                from_names: vec![],
                star: true,
                reexport,
            });
        }
        let mut names = vec![];
        loop {
            let n = self.expect_ident()?;
            let alias = if self.match_token(&Token::As) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            names.push((n, alias));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(Import {
            path,
            is_file,
            alias: None,
            kind: ImportKind::From,
            from_names: names,
            star: false,
            reexport,
        })
    }

    /// Parse `pub use m` / `pub use {a, b} from m` — a re-export. `pub`/`use`
    /// already consumed; this parses the target.
    fn parse_reexport(&mut self) -> Result<Import> {
        // `pub use {a, b} from m` — selective re-export from a module.
        if self.match_token(&Token::LBrace) {
            let mut names = vec![];
            loop {
                let n = self.expect_ident()?;
                let alias = if self.match_token(&Token::As) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                names.push((n, alias));
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RBrace)?;
            self.expect(Token::From)?;
            let (path, is_file) = self.parse_reexport_target()?;
            return Ok(Import {
                path,
                is_file,
                alias: None,
                kind: ImportKind::From,
                from_names: names,
                star: false,
                reexport: true,
            });
        }
        // `pub use m` — re-export the whole module (or `pub use m as n`).
        let mut imp = self.parse_import(true)?;
        imp.reexport = true;
        Ok(imp)
    }

    fn parse_reexport_target(&mut self) -> Result<(Vec<String>, bool)> {
        if let Some(Token::String(s)) = self.peek() {
            let s = s.clone();
            self.advance();
            return Ok((vec![s], true));
        }
        let mut path = vec![];
        if let Some(Token::Ident(name)) = self.peek() {
            path.push(name.clone());
            self.advance();
        } else {
            return Err(self.perr("Expected module name".to_string()));
        }
        while self.match_token(&Token::Dot) || self.match_token(&Token::ColonColon) {
            if let Some(Token::Ident(name)) = self.peek() {
                path.push(name.clone());
                self.advance();
            } else {
                return Err(self.perr("Expected identifier after path separator".to_string()));
            }
        }
        Ok((path, false))
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        if self.check(&Token::Pub) || self.check(&Token::Export) {
            self.advance();
            let inner = self.parse_stmt()?;
            return Ok(Stmt::Export(Box::new(inner)));
        }
        match self.peek() {
            Some(Token::Let) => self.parse_let(),
            Some(Token::If) => Ok(Stmt::Expr(Box::new(self.parse_if_expr()?))),
            Some(Token::Loop) => self.parse_loop(),
            Some(Token::While) => self.parse_while(),
            Some(Token::For) => self.parse_for(),
            Some(Token::Scan) => self.parse_scan(),
            Some(Token::Fetch) => self.parse_fetch(),
            Some(Token::Dump) => self.parse_dump(),
            Some(Token::Trace) => self.parse_trace(),
            Some(Token::Return) => self.parse_return(),
            Some(Token::Break) => {
                self.advance();
                self.semi()?;
                Ok(Stmt::Break)
            }
            Some(Token::Continue) => {
                self.advance();
                self.semi()?;
                Ok(Stmt::Continue)
            }
            Some(Token::Fn) => self.parse_fn(),
            Some(Token::Struct) => self.parse_struct(),
            Some(Token::Enum) => self.parse_enum(),
            Some(Token::Mod) => self.parse_mod(),
            Some(Token::Impl) => self.parse_impl(),
            Some(Token::Match) => Ok(Stmt::Expr(Box::new(self.parse_match_expr()?))),
            Some(Token::Trait) => self.parse_trait(),
            Some(Token::Try) => self.parse_try(),
            Some(Token::Raise) => {
                self.advance();
                let e = self.parse_expr()?;
                self.semi()?;
                Ok(Stmt::Raise(Box::new(e)))
            }
            Some(Token::Type) => self.parse_type_alias(),
            Some(Token::Extern) => self.parse_extern(),
            Some(Token::Macro) => self.parse_macro(),
            Some(Token::Const) => self.parse_const(),
            Some(Token::Async) => {
                // `async fn ...` -> async function; `async { ... }` -> async block.
                let is_fn = matches!(self.peek_n(1), Some(Token::Fn));
                if is_fn {
                    self.parse_fn()
                } else {
                    self.advance();
                    let body = self.parse_block()?;
                    Ok(Stmt::Async(body))
                }
            }
            Some(Token::BinStruct) => self.parse_binstruct(),
            _ => {
                let expr = self.parse_expr()?;
                self.semi()?;
                Ok(Stmt::Expr(Box::new(expr)))
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt> {
        self.expect(Token::Let)?;
        let mutable = self.match_token(&Token::Mut);

        if self.check(&Token::LParen) || self.check(&Token::LBracket) || self.check(&Token::LBrace) {
            let pattern = self.parse_pattern()?;
            let type_hint = if self.match_token(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(Token::Eq)?;
            let value = self.parse_expr()?;
            self.semi()?;
            return Ok(Stmt::Let {
                name: "_".to_string(),
                pattern: Some(pattern),
                mutable,
                value: Box::new(value),
                type_hint,
            });
        }

        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(self.perr("Expected variable name after 'let'".to_string())),
        };

        let type_hint = if self.match_token(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(Token::Eq)?;
        let value = self.parse_expr()?;
        self.semi()?;

        Ok(Stmt::Let {
            name,
            pattern: None,
            mutable,
            value: Box::new(value),
            type_hint,
        })
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        self.expect(Token::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        let else_branch = if self.match_token(&Token::Else) {
            if self.check(&Token::If) {
                let e = self.parse_if_expr()?;
                Some(vec![Stmt::Expr(Box::new(e))])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Expr::If {
            cond: Box::new(cond),
            then_branch,
            else_branch,
        })
    }

    fn parse_loop(&mut self) -> Result<Stmt> {
        self.expect(Token::Loop)?;
        let body = self.parse_block()?;
        Ok(Stmt::Loop(body))
    }

    fn parse_while(&mut self) -> Result<Stmt> {
        self.expect(Token::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While {
            cond: Box::new(cond),
            body,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt> {
        self.expect(Token::For)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(self.perr("Expected variable name after 'for'".to_string())),
        };
        self.expect(Token::In)?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            name,
            iterable: Box::new(iterable),
            body,
        })
    }

    fn parse_scan(&mut self) -> Result<Stmt> {
        self.expect(Token::Scan)?;
        let target = self.parse_expr()?;
        let options = if self.match_token(&Token::LBrace) {
            let opts = self.parse_options()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            vec![]
        };
        let body = if self.match_token(&Token::LBrace) {
            Some(self.parse_stmts_until_rbrace()?)
        } else {
            None
        };
        Ok(Stmt::Scan {
            target: Box::new(target),
            options,
            body,
        })
    }

    fn parse_fetch(&mut self) -> Result<Stmt> {
        self.expect(Token::Fetch)?;
        let target = self.parse_expr()?;
        let options = if self.match_token(&Token::LBrace) {
            let opts = self.parse_options()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            vec![]
        };
        let body = if self.match_token(&Token::LBrace) {
            Some(self.parse_stmts_until_rbrace()?)
        } else {
            None
        };
        Ok(Stmt::Fetch {
            target: Box::new(target),
            options,
            body,
        })
    }

    fn parse_dump(&mut self) -> Result<Stmt> {
        self.expect(Token::Dump)?;
        let value = self.parse_expr()?;
        let target = if self.match_token(&Token::Comma) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.semi()?;
        Ok(Stmt::Dump {
            value: Box::new(value),
            target,
        })
    }

    fn parse_trace(&mut self) -> Result<Stmt> {
        self.expect(Token::Trace)?;
        let value = self.parse_expr()?;
        self.semi()?;
        Ok(Stmt::Trace {
            value: Box::new(value),
        })
    }

    fn parse_return(&mut self) -> Result<Stmt> {
        self.expect(Token::Return)?;
        if self.match_token(&Token::Semi) {
            Ok(Stmt::Return(None))
        } else if self.check(&Token::RBrace) || self.peek().is_none() {
            Ok(Stmt::Return(None))
        } else {
            let expr = self.parse_expr()?;
            self.semi()?;
            Ok(Stmt::Return(Some(Box::new(expr))))
        }
    }

    fn parse_fn(&mut self) -> Result<Stmt> {
        let is_async = self.match_token(&Token::Async);
        self.expect(Token::Fn)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(self.perr("Expected function name".to_string())),
        };
        let (params, type_params) = self.parse_params_with_generics()?;
        let return_type = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let _ = type_params;
        Ok(Stmt::Let {
            name,
            pattern: None,
            mutable: false,
            value: Box::new(Expr::Function {
                params,
                return_type,
                body,
                captures: vec![],
                is_async,
            }),
            type_hint: None,
        })
    }

    fn parse_struct(&mut self) -> Result<Stmt> {
        self.expect(Token::Struct)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(Token::LBrace)?;
        let fields = self.parse_params()?;
        self.expect(Token::RBrace)?;
        Ok(Stmt::Struct {
            name,
            type_params,
            fields,
        })
    }

    fn parse_enum(&mut self) -> Result<Stmt> {
        self.expect(Token::Enum)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(Token::LBrace)?;
        let mut variants = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let vname = self.expect_ident()?;
            let fields = if self.match_token(&Token::LParen) {
                let mut fs = vec![];
                while !self.check(&Token::RParen) && self.peek().is_some() {
                    fs.push(self.parse_type()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                fs
            } else {
                vec![]
            };
            variants.push(EnumVariant { name: vname, fields });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Enum {
            name,
            type_params,
            variants,
        })
    }

    fn parse_mod(&mut self) -> Result<Stmt> {
        self.expect(Token::Mod)?;
        let name = self.expect_ident()?;
        let items = self.parse_block()?;
        Ok(Stmt::Mod { name, items })
    }

    fn parse_impl(&mut self) -> Result<Stmt> {
        self.expect(Token::Impl)?;
        let target = self.expect_ident()?;
        let trait_name = if self.match_token(&Token::For) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect(Token::LBrace)?;
        let mut methods = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            methods.push(self.parse_fn()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Impl {
            target,
            trait_name,
            methods,
        })
    }

    fn parse_trait(&mut self) -> Result<Stmt> {
        self.expect(Token::Trait)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut methods = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            self.expect(Token::Fn)?;
            let mname = self.expect_ident()?;
            self.expect(Token::LParen)?;
            let params = self.parse_params()?;
            self.expect(Token::RParen)?;
            let return_type = if self.match_token(&Token::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.semi()?;
            methods.push(TraitMethod {
                name: mname,
                params,
                return_type,
            });
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Trait { name, methods })
    }

    fn parse_try(&mut self) -> Result<Stmt> {
        self.expect(Token::Try)?;
        let body = self.parse_block()?;
        let catch_name;
        let catch_body;
        if self.match_token(&Token::Catch) {
            catch_name = if let Some(Token::Ident(n)) = self.peek() {
                let n = n.clone();
                self.advance();
                Some(n)
            } else {
                None
            };
            catch_body = self.parse_block()?;
        } else {
            catch_name = None;
            catch_body = vec![];
        }
        Ok(Stmt::Try {
            body,
            catch_name,
            catch_body,
        })
    }

    fn parse_type_alias(&mut self) -> Result<Stmt> {
        self.expect(Token::Type)?;
        let name = self.expect_ident()?;
        self.expect(Token::Eq)?;
        let alias = self.parse_type()?;
        self.semi()?;
        Ok(Stmt::TypeAlias { name, alias })
    }

    /// Parse `extern "C" [from "path"] { fn name(params) -> ret, ... }`.
    fn parse_extern(&mut self) -> Result<Stmt> {
        self.expect(Token::Extern)?;
        let abi = match self.peek() {
            Some(Token::String(s)) => {
                let s = s.clone();
                self.advance();
                s
            }
            _ => return Err(self.perr("Expected ABI string after 'extern' (e.g. \"C\")".to_string())),
        };
        if abi != "C" {
            return Err(self.perr(format!("Unsupported ABI '{}' (only \"C\" is supported)", abi)));
        }
        let lib = if matches!(self.peek(), Some(Token::From)) {
            self.advance();
            match self.peek() {
                Some(Token::String(s)) => {
                    let s = s.clone();
                    self.advance();
                    Some(s)
                }
                _ => return Err(self.perr("Expected library path string after 'from'".to_string())),
            }
        } else {
            None
        };
        self.expect(Token::LBrace)?;
        let mut decls = Vec::new();
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            self.expect(Token::Fn)?;
            let name = self.expect_ident()?;
            self.expect(Token::LParen)?;
            let mut params = Vec::new();
            let mut varargs = false;
            while !self.check(&Token::RParen) && self.peek().is_some() {
                if self.match_token(&Token::Ellipsis) {
                    varargs = true;
                    break;
                }
                let pname = self.expect_ident()?;
                let type_hint = if self.match_token(&Token::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(Param { name: pname, type_hint });
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RParen)?;
            let return_type = if self.match_token(&Token::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.match_token(&Token::Semi);
            self.match_token(&Token::Comma);
            decls.push(ForeignFn { name, params, varargs, return_type });
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Extern { abi, lib, decls })
    }

    /// Parse `macro name($params) { body }`. The body is a block whose
    /// statements may contain `$param` placeholders.
    fn parse_macro(&mut self) -> Result<Stmt> {
        self.expect(Token::Macro)?;
        let name = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        let body = self.parse_block()?;
        Ok(Stmt::MacroDef { name, params, body })
    }

    /// Parse `const NAME = expr`.
    fn parse_const(&mut self) -> Result<Stmt> {
        self.expect(Token::Const)?;
        let name = self.expect_ident()?;
        self.expect(Token::Eq)?;
        let value = self.parse_expr()?;
        self.semi()?;
        Ok(Stmt::Const { name, value: Box::new(value) })
    }

    /// Parse `binstruct Name { field: type, ... }`.
    fn parse_binstruct(&mut self) -> Result<Stmt> {
        self.expect(Token::BinStruct)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let fname = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let kind = self.parse_bin_kind()?;
            let repeat = if self.check(&Token::Colon) {
                self.advance();
                let e = self.parse_expr()?;
                Some(Box::new(e))
            } else {
                None
            };
            fields.push(BinField { name: fname, kind, repeat });
            self.match_token(&Token::Comma);
            self.match_token(&Token::Semi);
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::BinStructDef { name, fields })
    }

    /// Parse a `binstruct` field type: `u16be`, `u4`, `i32le`, `bytes`, `rest`,
    /// `bytes(4)`, or a nested binstruct name.
    fn parse_bin_kind(&mut self) -> Result<BinKind> {
        let name = self.expect_ident()?;
        if name == "rest" {
            return Ok(BinKind::Rest);
        }
        if name == "bytes" {
            if self.match_token(&Token::LParen) {
                let n = self.expect_int()?;
                self.expect(Token::RParen)?;
                return Ok(BinKind::Bytes(n as usize));
            }
            return Ok(BinKind::Bytes(0));
        }
        // `u16be` / `u16le` / `i8` / `u32` etc. — width + endianness.
        if let Some(rest) = name.strip_prefix(['u', 'i']) {
            let signed = name.starts_with('i');
            let (bits_str, endian) = if let Some(base) = rest.strip_suffix("le") {
                (base, Endian::Little)
            } else if let Some(base) = rest.strip_suffix("be") {
                (base, Endian::Big)
            } else {
                (rest, Endian::Big)
            };
            if let Ok(bits) = bits_str.parse::<u8>() {
                if bits > 0 && bits <= 64 && bits % 8 == 0 {
                    if signed {
                        return Ok(BinKind::Int { bits, endian });
                    } else {
                        return Ok(BinKind::Uint { bits, endian });
                    }
                }
            }
        }
        // Nested binstruct reference.
        Ok(BinKind::Ref(name))
    }

    fn expect_int(&mut self) -> Result<i64> {
        match self.peek().cloned() {
            Some(Token::Int(i)) => {
                self.advance();
                Ok(i)
            }
            Some(Token::Hex(h)) => {
                self.advance();
                Ok(h as i64)
            }
            _ => Err(self.perr("Expected integer".to_string())),
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                Ok(n)
            }
            _ => Err(self.perr("Expected identifier".to_string())),
        }
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>> {
        let mut tps = vec![];
        if self.match_token(&Token::Lt) {
            while !self.check(&Token::Gt) && self.peek().is_some() {
                tps.push(self.expect_ident()?);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt)?;
        }
        Ok(tps)
    }

    fn parse_params_with_generics(&mut self) -> Result<(Vec<Param>, Vec<String>)> {
        let type_params = if self.match_token(&Token::Lt) {
            let mut tps = vec![];
            while !self.check(&Token::Gt) && self.peek().is_some() {
                tps.push(self.expect_ident()?);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt)?;
            tps
        } else {
            vec![]
        };
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        Ok((params, type_params))
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>> {
        self.expect(Token::LBrace)?;
        self.parse_stmts_until_rbrace()
    }

    fn parse_stmts_until_rbrace(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_options(&mut self) -> Result<Vec<(String, Expr)>> {
        let mut options = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let key = match self.peek() {
                Some(Token::Ident(k)) => {
                    let k = k.clone();
                    self.advance();
                    k
                }
                _ => break,
            };
            self.expect(Token::Colon)?;
            let value = self.parse_expr()?;
            options.push((key, value));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(options)
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = vec![];
        while !self.check(&Token::RParen) && !self.check(&Token::RBrace) && self.peek().is_some() {
            let name = self.expect_ident()?;
            let type_hint = if self.match_token(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            params.push(Param { name, type_hint });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Type> {
        // Pointer types: `*u8`, `*i8`, `*void`, `*SomeType`.
        if self.check(&Token::Star) {
            self.advance();
            let inner = self.parse_type()?;
            return Ok(Type::Ptr(Box::new(inner)));
        }
        let base = match self.peek() {
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "void" => Type::Void,
                    "hex8" => Type::Hex(8),
                    "hex16" => Type::Hex(16),
                    "hex32" => Type::Hex(32),
                    "hex64" => Type::Hex(64),
                    "int" => Type::Int,
                    "i8" => Type::I8,
                    "i16" => Type::I16,
                    "i32" => Type::I32,
                    "i64" => Type::I64,
                    "u8" => Type::U8,
                    "u16" => Type::U16,
                    "u32" => Type::U32,
                    "u64" => Type::U64,
                    "f32" => Type::F32,
                    "f64" => Type::F64,
                    "string" => Type::String,
                    "bytes" => Type::Bytes,
                    "bool" => Type::Bool,
                    "nil" => Type::Nil,
                    "Option" => {
                        if self.match_token(&Token::Lt) {
                            let inner = self.parse_type()?;
                            self.expect(Token::Gt)?;
                            Type::Option(Box::new(inner))
                        } else {
                            Type::Custom(name)
                        }
                    }
                    "Result" => {
                        if self.match_token(&Token::Lt) {
                            let ok = self.parse_type()?;
                            self.expect(Token::Comma)?;
                            let err = self.parse_type()?;
                            self.expect(Token::Gt)?;
                            Type::Result(Box::new(ok), Box::new(err))
                        } else {
                            Type::Custom(name)
                        }
                    }
                    "evidence" => {
                        if self.match_token(&Token::Lt) {
                            let inner = self.parse_type()?;
                            self.expect(Token::Gt)?;
                            Type::Evidence(Box::new(inner))
                        } else {
                            Type::Custom(name)
                        }
                    }
                    _ => Type::Custom(name),
                }
            }
            _ => return Err(self.perr("Expected type".to_string())),
        };
        if self.match_token(&Token::LBracket) {
            self.expect(Token::RBracket)?;
            return Ok(Type::Array(Box::new(base)));
        }
        Ok(base)
    }

    fn parse_match_expr(&mut self) -> Result<Expr> {
        self.expect(Token::Match)?;
        let value = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        let mut arms = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let pattern = self.parse_pattern()?;
            let guard = if self.match_token(&Token::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::FatArrow)?;
            let body_expr = self.parse_expr()?;
            let body = vec![Stmt::Expr(Box::new(body_expr))];
            arms.push((pattern, guard, body));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        if self.match_token(&Token::Pipe) {
            let mut opts = vec![];
            loop {
                opts.push(self.parse_single_pattern()?);
                if !self.match_token(&Token::Pipe) {
                    break;
                }
            }
            return Ok(Pattern::Or(opts));
        }
        self.parse_single_pattern()
    }

    fn parse_single_pattern(&mut self) -> Result<Pattern> {
        match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                if n == "_" {
                    return Ok(Pattern::Wild);
                }
                if self.check(&Token::LParen) {
                    self.advance();
                    let mut inner = vec![];
                    while !self.check(&Token::RParen) && self.peek().is_some() {
                        inner.push(self.parse_pattern()?);
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RParen)?;
                    match n.as_str() {
                        "Some" => return Ok(Pattern::Some(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        "Ok" => return Ok(Pattern::Ok(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        "Err" => return Ok(Pattern::Err(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        _ => {}
                    }
                }
                if self.check(&Token::LBrace) {
                    self.advance();
                    let mut fields = vec![];
                    while !self.check(&Token::RBrace) && self.peek().is_some() {
                        let fname = self.expect_ident()?;
                        let fp = if self.match_token(&Token::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern::Ident(fname.clone())
                        };
                        fields.push((fname, fp));
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    return Ok(Pattern::Struct(n, fields));
                }
                match n.as_str() {
                    "None" => Ok(Pattern::None),
                    "Some" | "Ok" | "Err" => {
                        let inner = if self.match_token(&Token::LParen) {
                            let p = self.parse_pattern()?;
                            self.expect(Token::RParen)?;
                            Box::new(p)
                        } else {
                            Box::new(Pattern::Wild)
                        };
                        match n.as_str() {
                            "Some" => Ok(Pattern::Some(inner)),
                            "Ok" => Ok(Pattern::Ok(inner)),
                            "Err" => Ok(Pattern::Err(inner)),
                            _ => unreachable!(),
                        }
                    }
                    _ => Ok(Pattern::Ident(n)),
                }
            }
            Some(Token::Hex(h)) => {
                let h = *h;
                self.advance();
                Ok(Pattern::Hex(h))
            }
            Some(Token::Int(i)) => {
                let i = *i;
                self.advance();
                Ok(Pattern::Int(i))
            }
            Some(Token::Char(c)) => {
                let c = *c;
                self.advance();
                if c as u32 > 0xFF {
                    Err(self.perr("Char pattern out of byte range".to_string()))
                } else {
                    Ok(Pattern::Byte(c as u8))
                }
            }
            Some(Token::Float(f)) => {
                let f = *f;
                self.advance();
                Ok(Pattern::Ident(format!("{}", f)))
            }
            Some(Token::String(s)) => {
                let s = s.clone();
                self.advance();
                Ok(Pattern::String(s))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Pattern::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Pattern::Bool(false))
            }
            Some(Token::Nil) => {
                self.advance();
                Ok(Pattern::Nil)
            }
            Some(Token::LParen) => {
                self.advance();
                let mut inner = vec![];
                while !self.check(&Token::RParen) && self.peek().is_some() {
                    inner.push(self.parse_pattern()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                Ok(Pattern::Tuple(inner))
            }
            Some(Token::LBracket) => {
                self.advance();
                if self.peek_rest_in_brackets() {
                    return self.parse_bytes_pattern();
                }
                let mut inner = vec![];
                while !self.check(&Token::RBracket) && self.peek().is_some() {
                    inner.push(self.parse_pattern()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket)?;
                Ok(Pattern::Array(inner))
            }
            _ => Err(self.perr("Expected pattern".to_string())),
        }
    }

    /// True if the bracket list starting at the current position contains a
    /// top-level `..` rest marker, signalling a binary bytes pattern.
    fn peek_rest_in_brackets(&self) -> bool {
        let mut depth = 1usize;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i].0 {
                Token::LBracket => depth += 1,
                Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Token::DotDot if depth == 1 => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Parse a binary bytes pattern `[b0, b1, ..]` where each element is a byte
    /// literal (hex, int, or char) and a trailing `..` matches the remainder.
    fn parse_bytes_pattern(&mut self) -> Result<Pattern> {
        let mut parts: Vec<BytesPat> = vec![];
        while !self.check(&Token::RBracket) && self.peek().is_some() {
            if self.match_token(&Token::DotDot) {
                parts.push(BytesPat::Rest);
                break;
            }
            let byte = match self.peek().cloned() {
                Some(Token::Hex(h)) => {
                    self.advance();
                    if h > 0xFF {
                        return Err(self.perr("Byte value out of range in bytes pattern".to_string()));
                    }
                    h as u8
                }
                Some(Token::Int(i)) => {
                    self.advance();
                    if !(0..=255).contains(&i) {
                        return Err(self.perr("Byte value out of range in bytes pattern".to_string()));
                    }
                    i as u8
                }
                Some(Token::Char(c)) => {
                    self.advance();
                    if c as u32 > 0xFF {
                        return Err(self.perr("Char out of byte range in bytes pattern".to_string()));
                    }
                    c as u8
                }
                _ => return Err(self.perr("Expected byte literal in bytes pattern".to_string())),
            };
            parts.push(BytesPat::Byte(byte));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBracket)?;
        Ok(Pattern::Bytes(parts))
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Result<Expr> {
        let mut left = self.parse_assignment()?;
        while self.match_token(&Token::PipeGt) {
            let right = self.parse_assignment()?;
            left = desugar_pipe(left, right);
        }
        Ok(left)
    }

    fn parse_assignment(&mut self) -> Result<Expr> {
        let mut expr = self.parse_or()?;
        if self.match_token(&Token::Eq) {
            let value = self.parse_expr()?;
            match expr {
                Expr::Ident(name) => expr = Expr::Assign(name, Box::new(value)),
                Expr::Index(obj, idx) => {
                    expr = Expr::IndexAssign {
                        obj,
                        idx,
                        value: Box::new(value),
                    };
                }
                Expr::FieldAccess(obj, field) => {
                    expr = Expr::FieldAssign {
                        obj,
                        field,
                        value: Box::new(value),
                    };
                }
                _ => return Err(self.perr("Invalid assignment target".to_string())),
            }
        } else if self.match_token(&Token::PlusEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Add, name, Box::new(value));
            }
        } else if self.match_token(&Token::MinusEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Sub, name, Box::new(value));
            }
        } else if self.match_token(&Token::StarEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Mul, name, Box::new(value));
            }
        } else if self.match_token(&Token::SlashEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Div, name, Box::new(value));
            }
        } else if self.match_token(&Token::PercentEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Rem, name, Box::new(value));
            }
        }
        if self.match_token(&Token::Question) {
            expr = Expr::TryExpr(Box::new(expr));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.match_token(&Token::OrOr) {
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_equality()?;
        while self.match_token(&Token::AndAnd) {
            let right = self.parse_equality()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            if self.match_token(&Token::EqEq) {
                let right = self.parse_comparison()?;
                left = Expr::Binary(BinOp::Eq, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::NotEq) {
                let right = self.parse_comparison()?;
                left = Expr::Binary(BinOp::NotEq, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_range()?;
        loop {
            if self.match_token(&Token::Lt) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::Lt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Gt) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::Gt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::LtEq) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::LtEq, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::GtEq) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::GtEq, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_range(&mut self) -> Result<Expr> {
        let left = self.parse_bitwise()?;
        if self.match_token(&Token::DotDot) {
            if self.check(&Token::RBracket)
                || self.check(&Token::RParen)
                || self.check(&Token::Comma)
                || self.check(&Token::RBrace)
                || self.check(&Token::Semi)
                || self.peek().is_none()
            {
                return Ok(Expr::Range(Some(Box::new(left)), None));
            }
            let right = self.parse_bitwise()?;
            Ok(Expr::Range(Some(Box::new(left)), Some(Box::new(right))))
        } else {
            Ok(left)
        }
    }

    fn parse_bitwise(&mut self) -> Result<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            if self.match_token(&Token::Ampersand) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitAnd, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Pipe) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitOr, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Caret) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitXor, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr> {
        let mut left = self.parse_term()?;
        loop {
            if self.match_token(&Token::Shl) {
                let right = self.parse_term()?;
                left = Expr::Binary(BinOp::Shl, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Shr) {
                let right = self.parse_term()?;
                left = Expr::Binary(BinOp::Shr, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr> {
        let mut left = self.parse_factor()?;
        loop {
            if self.match_token(&Token::Plus) {
                let right = self.parse_factor()?;
                left = Expr::Binary(BinOp::Add, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Minus) {
                let right = self.parse_factor()?;
                left = Expr::Binary(BinOp::Sub, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            if self.match_token(&Token::Star) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Mul, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Slash) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Div, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Percent) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Rem, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.match_token(&Token::Minus) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::Minus, Box::new(expr)))
        } else if self.match_token(&Token::Bang) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::Not, Box::new(expr)))
        } else if self.match_token(&Token::Tilde) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::BitNot, Box::new(expr)))
        } else if self.match_token(&Token::Await) {
            let expr = self.parse_unary()?;
            Ok(Expr::Await(Box::new(expr)))
        } else if self.match_token(&Token::Spawn) {
            let expr = self.parse_unary()?;
            Ok(Expr::Spawn(Box::new(expr)))
        } else if self.match_token(&Token::Raise) {
            let expr = self.parse_unary()?;
            Ok(Expr::Raise(Box::new(expr)))
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            // `name!(args)` macro invocation (only when the base is an ident).
            if self.check(&Token::Bang) {
                if let Expr::Ident(name) = &expr {
                    let name = name.clone();
                    self.advance(); // consume `!`
                    self.expect(Token::LParen)?;
                    let args = self.parse_args()?;
                    self.expect(Token::RParen)?;
                    expr = Expr::MacroInvoke { name, args };
                    continue;
                }
            }
            if self.match_token(&Token::LParen) {
                let args = self.parse_args()?;
                self.expect(Token::RParen)?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.match_token(&Token::Dot) {
                if matches!(self.peek(), Some(Token::Int(_))) {
                    if let Some(Token::Int(i)) = self.peek() {
                        let i = *i;
                        self.advance();
                        expr = Expr::Index(Box::new(expr), Box::new(Expr::Int(i)));
                        continue;
                    }
                }
                let field = self.expect_ident()?;
                expr = Expr::FieldAccess(Box::new(expr), field);
            } else if self.match_token(&Token::ColonColon) {
                let seg = self.expect_ident()?;
                let mut path = match expr {
                    Expr::Path(p) => p,
                    Expr::Ident(n) => vec![n],
                    other => return Err(self.perr(format!("Invalid path base: {:?}", other))),
                };
                path.push(seg);
                expr = Expr::Path(path);
            } else if self.match_token(&Token::LBracket) {
                let idx = self.parse_expr()?;
                self.expect(Token::RBracket)?;
                expr = Expr::Index(Box::new(expr), Box::new(idx));
            } else if self.match_token(&Token::Question) {
                expr = Expr::TryExpr(Box::new(expr));
            } else if self.match_token(&Token::As) {
                let t = self.parse_type()?;
                expr = Expr::As(Box::new(expr), t);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = vec![];
        while !self.check(&Token::RParen) && self.peek().is_some() {
            args.push(self.parse_expr()?);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().cloned() {
            Some(Token::MacroVar(n)) => {
                self.advance();
                Ok(Expr::MacroVar(n))
            }
            Some(Token::Hex(h)) => {
                self.advance();
                Ok(Expr::Hex(h))
            }
            Some(Token::Int(i)) => {
                self.advance();
                Ok(Expr::Int(i))
            }
            Some(Token::Float(f)) => {
                self.advance();
                Ok(Expr::Float(f))
            }
            Some(Token::Float32(f)) => {
                self.advance();
                Ok(Expr::Float32(f))
            }
            Some(Token::TypedInt(d)) => {
                let d = d.clone();
                self.advance();
                Ok(Expr::TypedInt(d.value, map_int_kind(d.kind)))
            }
            Some(Token::String(s)) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Some(Token::Interp(template)) => {
                self.advance();
                let parts = parse_interp_parts(&template)?;
                Ok(Expr::Interp {
                    template,
                    parts,
                })
            }
            Some(Token::Bytes(b)) => {
                self.advance();
                Ok(Expr::Bytes(b))
            }
            Some(Token::Char(c)) => {
                self.advance();
                Ok(Expr::Int(c as i64))
            }
            Some(Token::Regex((pattern, flags))) => {
                let pattern = pattern.clone();
                let flags = flags.clone();
                self.advance();
                Ok(Expr::Regex(pattern, flags))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Some(Token::Nil) => {
                self.advance();
                Ok(Expr::Nil)
            }
            Some(Token::If) => self.parse_if_expr(),
            Some(Token::Match) => self.parse_match_expr(),
            Some(Token::Fn) => self.parse_lambda(),
            Some(Token::Evidence) => self.parse_evidence(),
            Some(Token::LParen) => {
                self.advance();
                if self.check(&Token::RParen) {
                    self.advance();
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.parse_expr()?;
                if self.match_token(&Token::Comma) {
                    let mut elems = vec![first];
                    while !self.check(&Token::RParen) && self.peek().is_some() {
                        elems.push(self.parse_expr()?);
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expr::Tuple(elems))
                } else {
                    self.expect(Token::RParen)?;
                    Ok(first)
                }
            }
            Some(Token::LBracket) => {
                self.advance();
                let mut elements = vec![];
                while !self.check(&Token::RBracket) && self.peek().is_some() {
                    elements.push(self.parse_expr()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket)?;
                Ok(Expr::Array(elements))
            }
            Some(Token::LBrace) => self.parse_map_or_block(),
            Some(Token::Ident(name)) => {
                self.advance();
                if self.check(&Token::ColonColon) {
                    let mut path = vec![name];
                    while self.match_token(&Token::ColonColon) {
                        path.push(self.expect_ident()?);
                    }
                    return Ok(Expr::Path(path));
                }
                if self.check(&Token::LBrace) && self.looks_like_struct_lit() {
                    self.advance();
                    let mut fields = vec![];
                    while !self.check(&Token::RBrace) && self.peek().is_some() {
                        let fname = self.expect_ident()?;
                        self.expect(Token::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    return Ok(Expr::StructLit { name, fields });
                }
                Ok(Expr::Ident(name))
            }
            Some(tok) => Err(self.perr(format!("Unexpected token in expression: {:?}", tok))),
            None => Err(self.perr("Unexpected end of input".to_string())),
        }
    }

    fn looks_like_struct_lit(&self) -> bool {
        if self.check(&Token::LBrace) {
            if self.peek_n(1).map(|t| matches!(t, Token::RBrace)).unwrap_or(false) {
                return true;
            }
            matches!(self.peek_n(1), Some(Token::Ident(_)))
                && matches!(self.peek_n(2), Some(Token::Colon))
        } else {
            false
        }
    }

    fn parse_map_or_block(&mut self) -> Result<Expr> {
        self.advance();
        if self.check(&Token::RBrace) {
            self.advance();
            return Ok(Expr::Map(vec![]));
        }
        if matches!(self.peek(), Some(Token::Ident(_)))
            && matches!(self.peek_n(1), Some(Token::Colon))
        {
            let mut pairs = vec![];
            while !self.check(&Token::RBrace) && self.peek().is_some() {
                let key = match self.peek() {
                    Some(Token::Ident(name)) => {
                        let name = name.clone();
                        self.advance();
                        Expr::String(name)
                    }
                    _ => self.parse_expr()?,
                };
                self.expect(Token::Colon)?;
                let value = self.parse_expr()?;
                pairs.push((key, value));
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RBrace)?;
            return Ok(Expr::Map(pairs));
        }
        let mut stmts = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        if stmts.len() == 1 {
            if let Stmt::Expr(e) = &stmts[0] {
                return Ok((**e).clone());
            }
        }
        Ok(Expr::Block(stmts))
    }

    fn parse_lambda(&mut self) -> Result<Expr> {
        self.expect(Token::Fn)?;
        let is_async = self.match_token(&Token::Async);
        let (params, _) = self.parse_params_with_generics()?;
        let return_type = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let _ = return_type;
        if self.check(&Token::LBrace) {
            let body = self.parse_block()?;
            Ok(Expr::Function {
                params,
                return_type: None,
                body,
                captures: vec![],
                is_async,
            })
        } else {
            let body_expr = self.parse_expr()?;
            Ok(Expr::Lambda {
                params,
                body: Box::new(body_expr),
                captures: vec![],
            })
        }
    }

    /// Parse `evidence<T> from expr`. The concrete inner type `T` is parsed and
    /// discarded for now (the runtime tag carries the source); the value and
    /// its source expression are the payload users interact with.
    fn parse_evidence(&mut self) -> Result<Expr> {
        self.expect(Token::Evidence)?;
        if self.match_token(&Token::Lt) {
            let _ = self.parse_type()?;
            self.expect(Token::Gt)?;
        }
        self.expect(Token::From)?;
        let value = self.parse_expr()?;
        Ok(Expr::EvidenceFrom {
            value: Box::new(value),
        })
    }
}

/// Desugar the pipeline operator `a |> b` into a call expression.
///
/// `x |> f`              becomes  `f(x)`
/// `x |> f(a, b)`         becomes  `f(x, a, b)`
/// `x |> SomeIdent`       becomes  `SomeIdent(x)`
fn desugar_pipe(left: Expr, right: Expr) -> Expr {
    match right {
        Expr::Call { callee, mut args } => {
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(left);
            new_args.append(&mut args);
            Expr::Call { callee, args: new_args }
        }
        other => Expr::Call { callee: Box::new(other), args: vec![left] },
    }
}

fn parse_interp_parts(template: &str) -> Result<Vec<Expr>> {
    let mut parts = vec![];
    let mut chars = template.chars().peekable();
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                buf.push('{');
                continue;
            }
            let mut depth = 1;
            let mut expr_src = String::new();
            while let Some(c2) = chars.next() {
                if c2 == '{' {
                    depth += 1;
                    expr_src.push(c2);
                } else if c2 == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    expr_src.push(c2);
                } else {
                    expr_src.push(c2);
                }
            }
            if !expr_src.is_empty() {
                parts.push(parse_expr_str(&expr_src)?);
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
                buf.push('}');
                continue;
            }
            buf.push(c);
        } else {
            buf.push(c);
        }
    }
    let _ = buf;
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn test_parse_simple_let() {
        let source = "let x = 0x1A;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        assert_eq!(module.items.len(), 1);
        match &module.items[0] {
            Stmt::Let { name, value, .. } => {
                assert_eq!(name, "x");
                assert!(matches!(**value, Expr::Hex(0x1A)));
            }
            _ => panic!("Expected let statement"),
        }
    }

    #[test]
    fn test_parse_binop() {
        let source = "let sig = 0xDEAD & 0xFF00;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => {
                assert!(matches!(**value, Expr::Binary(BinOp::BitAnd, _, _)));
            }
            _ => panic!("Expected let statement"),
        }
    }

    #[test]
    fn test_parse_scan() {
        let source = "scan target { range: [0x0010, 0x0050] }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        assert_eq!(module.items.len(), 1);
        assert!(matches!(&module.items[0], Stmt::Scan { .. }));
    }

    #[test]
    fn test_parse_float() {
        let source = "let x = 3.14;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::Float(_))),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_tuple() {
        let source = "let p = (1, 2, 3);";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(&**value, Expr::Tuple(t) if t.len() == 3)),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_interp() {
        let source = "let x = f\"hello {name}!\";";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::Interp { .. })),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_if_expr() {
        let source = "let x = if true { 1 } else { 2 };";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::If { .. })),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_struct_lit() {
        let source = "let p = Point { x: 1, y: 2 };";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::StructLit { .. })),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_pipeline_desugars_to_call() {
        let source = "let y = x |> f;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => match &**value {
                Expr::Call { callee, args } => {
                    assert!(matches!(callee.as_ref(), Expr::Ident(n) if n == "f"));
                    assert_eq!(args.len(), 1);
                    assert!(matches!(&args[0], Expr::Ident(n) if n == "x"));
                }
                other => panic!("expected Call, got {:?}", other),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_pipeline_with_args() {
        let source = "let y = x |> f(1, 2);";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => match &**value {
                Expr::Call { args, .. } => assert_eq!(args.len(), 3),
                other => panic!("expected Call, got {:?}", other),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_regex_literal() {
        let source = "let r = /\\d+/g;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => match &**value {
                Expr::Regex(pat, flags) => {
                    assert_eq!(pat, "\\d+");
                    assert_eq!(flags, "g");
                }
                other => panic!("expected Regex, got {:?}", other),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_bytes_pattern_rest() {
        let source = "match b { [0x89, 'P', 'N', 'G', ..] => { dump \"png\" } }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        // Statement-level `match` is wrapped in Stmt::Expr(Expr::Match).
        let arms = match &module.items[0] {
            Stmt::Expr(e) => match &**e {
                Expr::Match { arms, .. } => arms,
                other => panic!("expected Expr::Match, got {:?}", other),
            },
            other => panic!("expected Stmt::Expr, got {:?}", other),
        };
        match &arms[0].0 {
            Pattern::Bytes(parts) => {
                assert_eq!(parts.len(), 5);
                assert!(matches!(&parts[0], BytesPat::Byte(0x89)));
                assert!(matches!(&parts[1], BytesPat::Byte(b'P')));
                assert!(matches!(&parts[4], BytesPat::Rest));
            }
            other => panic!("expected Bytes pattern, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_char_expr() {
        let source = "let c = 'A';";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(&**value, Expr::Int(65))),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_extern_block() {
        let source = "extern \"C\" { fn printf(fmt: *u8, ...) -> i32; fn getpid() -> i32 }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Extern { abi, lib, decls } => {
                assert_eq!(abi, "C");
                assert!(lib.is_none());
                assert_eq!(decls.len(), 2);
                assert_eq!(decls[0].name, "printf");
                assert!(decls[0].varargs);
                assert!(matches!(decls[0].return_type, Some(Type::I32)));
                assert!(matches!(decls[0].params[0].type_hint, Some(Type::Ptr(_))));
                assert_eq!(decls[1].name, "getpid");
                assert!(!decls[1].varargs);
            }
            _ => panic!("expected Stmt::Extern"),
        }
    }

    #[test]
    fn test_parse_extern_with_from() {
        let source = "extern \"C\" from \"libc.so.6\" { fn abs(n: i32) -> i32 }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Extern { abi, lib, decls } => {
                assert_eq!(abi, "C");
                assert_eq!(lib.as_deref(), Some("libc.so.6"));
                assert_eq!(decls.len(), 1);
            }
            _ => panic!("expected Stmt::Extern"),
        }
    }

    #[test]
    fn test_parse_macro_def_and_invoke() {
        let source = "macro add1(x: expr) { $x + 1 } dump add1!(41)";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::MacroDef { name, params, body } => {
                assert_eq!(name, "add1");
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert!(!body.is_empty());
            }
            _ => panic!("expected Stmt::MacroDef"),
        }
        match &module.items[1] {
            Stmt::Dump { value, .. } => match &**value {
                Expr::MacroInvoke { name, args } => {
                    assert_eq!(name, "add1");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!("expected Expr::MacroInvoke"),
            },
            _ => panic!("expected Stmt::Dump"),
        }
    }

    #[test]
    fn test_parse_import_forms() {
        let cases = vec![
            ("import math", ImportKind::Whole, false, false, 0, false, false),
            ("import math as m", ImportKind::Whole, false, false, 0, false, false),
            ("import \"./m.rak\"", ImportKind::Whole, true, false, 0, false, false),
            ("from math import add", ImportKind::From, false, false, 1, false, false),
            ("from math import add as plus, mul as times", ImportKind::From, false, false, 2, false, false),
            ("from math import *", ImportKind::From, false, false, 0, true, false),
            ("pub use math", ImportKind::Whole, false, false, 0, false, true),
            ("pub use {add, mul} from math", ImportKind::From, false, false, 2, false, true),
        ];
        for (src, kind, is_file, _has_alias, n_names, star, reexport) in cases {
            let tokens = tokenize(src).unwrap();
            let module = parse(&tokens, src).unwrap();
            assert_eq!(module.imports.len(), 1, "for {:?}", src);
            let imp = &module.imports[0];
            assert_eq!(imp.kind, kind, "kind for {:?}", src);
            assert_eq!(imp.is_file, is_file, "is_file for {:?}", src);
            assert_eq!(imp.from_names.len(), n_names, "names for {:?}", src);
            assert_eq!(imp.star, star, "star for {:?}", src);
            assert_eq!(imp.reexport, reexport, "reexport for {:?}", src);
        }
    }

    #[test]
    fn test_parse_export_keyword_alias_for_pub() {
        let source = "export fn add(a, b) { return a + b }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens, source).unwrap();
        match &module.items[0] {
            Stmt::Export(inner) => match inner.as_ref() {
                Stmt::Let { name, .. } => assert_eq!(name, "add"),
                _ => panic!("expected Export(Let)"),
            },
            _ => panic!("expected Stmt::Export"),
        }
    }
}
