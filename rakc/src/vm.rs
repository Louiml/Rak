use crate::bytecode::{Chunk, Op};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ChannelHandle {
    pub id: u64,
}
pub struct FutureHandle {
    pub id: u64,
}

pub struct Vm {
    pub globals: HashMap<String, Value>,
    output: Vec<String>,
}

impl Vm {
    pub fn new() -> Self {
        let mut vm = Vm {
            globals: HashMap::new(),
            output: Vec::new(),
        };
        vm.register_natives();
        vm
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    fn register_natives(&mut self) {
        self.insert_native("len", |args| {
            match args.first() {
                Some(Value::String(s)) => Ok(Value::I64(s.chars().count() as i64)),
                Some(Value::Array(a)) => Ok(Value::I64(a.len() as i64)),
                Some(Value::Tuple(t)) => Ok(Value::I64(t.len() as i64)),
                Some(Value::Bytes(b)) => Ok(Value::I64(b.len() as i64)),
                Some(Value::Map(m)) => Ok(Value::I64(m.len() as i64)),
                _ => Err("len() requires string/array/tuple/bytes/map".to_string()),
            }
        });
        self.insert_native("int", |args| match args.first() {
            Some(Value::I64(i)) => Ok(Value::I64(*i)),
            Some(Value::Hex(h, _)) => Ok(Value::I64(*h as i64)),
            Some(Value::F64(f)) => Ok(Value::I64(*f as i64)),
            Some(Value::String(s)) => s.parse::<i64>().map(Value::I64).map_err(|_| "int() parse error".to_string()),
            Some(Value::Bool(b)) => Ok(Value::I64(if *b { 1 } else { 0 })),
            _ => Ok(Value::I64(0)),
        });
        self.insert_native("float", |args| Ok(Value::F64(args.first().and_then(|v| v.as_f64()).unwrap_or(0.0))));
        self.insert_native("string", |args| Ok(Value::String(Arc::from(args.first().map(|v| v.to_string()).unwrap_or_default().as_str()))));
        self.insert_native("upper", |args| Ok(Value::String(Arc::from(native_str(args.first()).to_uppercase().as_str()))));
        self.insert_native("lower", |args| Ok(Value::String(Arc::from(native_str(args.first()).to_lowercase().as_str()))));
        self.insert_native("md5", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::md5(&data).as_str())))
        });
        self.insert_native("sha1", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::sha1(&data).as_str())))
        });
        self.insert_native("sha256", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::sha256(&data).as_str())))
        });
        self.insert_native("hex_encode", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::hex_encode(&data).as_str())))
        });
        self.insert_native("base64_encode", |args| {
            let data = native_bytes(args.first());
            Ok(Value::String(Arc::from(rak_stdlib::base64_encode(&data).as_str())))
        });
        self.insert_native("fmt", |args| {
            let fmt = native_str(args.first());
            let rest: Vec<Value> = args.iter().skip(1).cloned().collect();
            Ok(Value::String(Arc::from(format_rak(&fmt, &rest).as_str())))
        });
        self.insert_native("to_hex", |args| {
            Ok(Value::String(Arc::from(format!("0x{:X}", args.first().and_then(|v| v.as_u64()).unwrap_or(0)).as_str())))
        });
    }

    fn insert_native(&mut self, name: &str, f: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static) {
        self.globals.insert(name.to_string(), Value::NativeFn(Arc::from(name), Arc::new(f)));
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<Vec<String>, String> {
        let mut frame = Frame { code: chunk, ip: 0, stack: Vec::new(), locals: Vec::new() };
        self.exec_frame(&mut frame)?;
        Ok(std::mem::take(&mut self.output))
    }

    fn exec_frame(&mut self, frame: &mut Frame) -> Result<(), String> {
        while frame.ip < frame.code.code.len() {
            let op = Op::from_u8(frame.code.code[frame.ip]).ok_or_else(|| format!("bad opcode at {}", frame.ip))?;
            frame.ip += 1;
            match op {
                Op::Nop => {}
                Op::LoadConst => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    frame.push(frame.code.constants[ci].clone());
                }
                Op::LoadLocal => {
                    let slot = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    frame.push(frame.locals[slot].clone());
                }
                Op::StoreLocal => {
                    let slot = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let v = frame.pop();
                    if slot < frame.locals.len() {
                        frame.locals[slot] = v;
                    } else {
                        while frame.locals.len() < slot {
                            frame.locals.push(Value::Nil);
                        }
                        frame.locals.push(v);
                    }
                }
                Op::LoadGlobal => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let name = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad global name".to_string()),
                    };
                    match self.globals.get(&name) {
                        Some(v) => frame.push(v.clone()),
                        None => return Err(format!("Undefined: {}", name)),
                    }
                }
                Op::StoreGlobal => {
                    let ci = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    let name = match &frame.code.constants[ci] {
                        Value::String(s) => s.to_string(),
                        _ => return Err("bad global name".to_string()),
                    };
                    let v = frame.pop();
                    self.globals.insert(name, v);
                }
                Op::Pop => { frame.pop(); }
                Op::Dup => { let v = frame.peek().clone(); frame.push(v); }
                Op::AddI => bin_int(frame, |a, b| a.wrapping_add(b), |a, b| a + b),
                Op::SubI => bin_int(frame, |a, b| a.wrapping_sub(b), |a, b| a - b),
                Op::MulI => bin_int(frame, |a, b| a.wrapping_mul(b), |a, b| a * b),
                Op::DivI => {
                    let r = frame.pop(); let l = frame.pop();
                    match (l, r) {
                        (Value::I64(a), Value::I64(b)) => if b == 0 { return Err("div by zero".to_string()); } else { frame.push(Value::I64(a / b)); },
                        (Value::F64(a), Value::F64(b)) => if b == 0.0 { return Err("div by zero".to_string()); } else { frame.push(Value::F64(a / b)); },
                        (a, b) => { let av = a.as_f64().unwrap_or(0.0); let bv = b.as_f64().unwrap_or(0.0); if bv == 0.0 { return Err("div by zero".to_string()); } frame.push(Value::F64(av / bv)); },
                    }
                }
                Op::RemI => {
                    let r = frame.pop(); let l = frame.pop();
                    match (l, r) {
                        (Value::I64(a), Value::I64(b)) => if b == 0 { return Err("rem by zero".to_string()); } else { frame.push(Value::I64(a % b)); },
                        (Value::F64(a), Value::F64(b)) => frame.push(Value::F64(a % b)),
                        (a, b) => { let av = a.as_f64().unwrap_or(0.0); let bv = b.as_f64().unwrap_or(0.0); frame.push(Value::F64(av % bv)); },
                    }
                }
                Op::NegI => {
                    let v = frame.pop();
                    match v {
                        Value::I64(i) => frame.push(Value::I64(-i)),
                        Value::F64(f) => frame.push(Value::F64(-f)),
                        other => frame.push(Value::F64(-(other.as_f64().unwrap_or(0.0)))),
                    }
                }
                Op::AddF => bin_float(frame, |a, b| a + b),
                Op::SubF => bin_float(frame, |a, b| a - b),
                Op::MulF => bin_float(frame, |a, b| a * b),
                Op::DivF => bin_float(frame, |a, b| if b == 0.0 { 0.0 } else { a / b }),
                Op::NegF => { let v = frame.pop(); frame.push(Value::F64(-(v.as_f64().unwrap_or(0.0)))); }
                Op::BitAnd => bin_int(frame, |a, b| a & b, |_, _| 0.0),
                Op::BitOr => bin_int(frame, |a, b| a | b, |_, _| 0.0),
                Op::BitXor => bin_int(frame, |a, b| a ^ b, |_, _| 0.0),
                Op::BitNot => { let v = frame.pop(); frame.push(Value::I64(!(v.as_i64().unwrap_or(0)))); }
                Op::Shl => bin_int(frame, |a, b| a << b, |_, _| 0.0),
                Op::Shr => bin_int(frame, |a, b| a >> b, |_, _| 0.0),
                Op::Eq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l == r)); }
                Op::NotEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l != r)); }
                Op::Lt => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) < r.as_i64().unwrap_or(0))); }
                Op::Gt => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) > r.as_i64().unwrap_or(0))); }
                Op::LtEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) <= r.as_i64().unwrap_or(0))); }
                Op::GtEq => { let r = frame.pop(); let l = frame.pop(); frame.push(Value::Bool(l.as_i64().unwrap_or(0) >= r.as_i64().unwrap_or(0))); }
                Op::Not => { let v = frame.pop(); frame.push(Value::Bool(!v.is_truthy())); }
                Op::True => frame.push(Value::Bool(true)),
                Op::False => frame.push(Value::Bool(false)),
                Op::Nil => frame.push(Value::Nil),
                Op::Jump => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip = target;
                }
                Op::JumpIfFalse => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    if !frame.peek().is_truthy() {
                        frame.ip = target;
                    }
                }
                Op::JumpIfTrue => {
                    let target = frame.code.read_u16(frame.ip) as usize;
                    frame.ip += 2;
                    if frame.peek().is_truthy() {
                        frame.ip = target;
                    }
                }
                Op::Call => {
                    let argc = frame.code.code[frame.ip] as usize;
                    frame.ip += 1;
                    let mut args: Vec<Value> = (0..argc).map(|_| frame.pop()).collect();
                    args.reverse();
                    let callee = frame.pop();
                    match callee {
                        Value::NativeFn(name, f) => {
                            let result = f(&args).map_err(|e| format!("{}: {}", name, e))?;
                            frame.push(result);
                        }
                        Value::Closure { code, nparams, .. } => {
                            let mut sub = Frame { code: &code, ip: 0, stack: Vec::new(), locals: Vec::with_capacity(nparams) };
                            for i in 0..nparams {
                                sub.locals.push(args.get(i).cloned().unwrap_or(Value::Nil));
                            }
                            self.exec_frame(&mut sub)?;
                            frame.push(sub.stack.pop().unwrap_or(Value::Nil));
                        }
                        _ => return Err("cannot call non-function".to_string()),
                    }
                }
                Op::Return => {
                    return Ok(());
                }
                Op::Print => {
                    let v = frame.pop();
                    self.output.push(format!("[DUMP] {}", v));
                }
                Op::Trace => {
                    let v = frame.pop();
                    self.output.push(format!("[TRACE] {:?}", v));
                }
                Op::NewArray => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(Value::Array(Arc::from(items)));
                }
                Op::NewTuple => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(Value::Tuple(Arc::from(items)));
                }
                Op::NewMap => {
                    let n = match frame.pop() { Value::I64(n) => n as usize, _ => 0 };
                    let mut m = HashMap::new();
                    for _ in 0..n {
                        let v = frame.pop();
                        let k = frame.pop();
                        m.insert(k.to_string(), v);
                    }
                    frame.push(Value::Map(Arc::from(m)));
                }
                Op::IndexGet => {
                    let idx = frame.pop();
                    let obj = frame.pop();
                    match (&obj, &idx) {
                        (Value::Array(a), Value::I64(i)) => {
                            frame.push(a.get(*i as usize).cloned().unwrap_or(Value::Nil));
                        }
                        (Value::String(s), Value::I64(i)) => {
                            frame.push(s.chars().nth(*i as usize).map(|c| Value::String(Arc::from(c.to_string().as_str()))).unwrap_or(Value::Nil));
                        }
                        _ => { frame.push(Value::Nil); }
                    }
                }
                Op::FieldGet => { frame.pop(); frame.push(Value::Nil); }
                Op::Closure => {}
                Op::GetUpvalue | Op::SetUpvalue => {}
                Op::LoopBegin | Op::LoopEnd => {}
            }
        }
        Ok(())
    }
}

struct Frame<'a> {
    code: &'a Chunk,
    ip: usize,
    stack: Vec<Value>,
    locals: Vec<Value>,
}

impl<'a> Frame<'a> {
    fn push(&mut self, v: Value) { self.stack.push(v); }
    fn pop(&mut self) -> Value { self.stack.pop().unwrap_or(Value::Nil) }
    fn peek(&self) -> &Value { self.stack.last().unwrap_or(&Value::Nil) }
}

fn bin_int(frame: &mut Frame, fi: impl Fn(i64, i64) -> i64, ff: impl Fn(f64, f64) -> f64) {
    let r = frame.pop();
    let l = frame.pop();
    match (l, r) {
        (Value::I64(a), Value::I64(b)) => frame.push(Value::I64(fi(a, b))),
        (Value::F64(a), Value::F64(b)) => frame.push(Value::F64(ff(a, b))),
        (a, b) => {
            let av = a.as_f64().unwrap_or(0.0);
            let bv = b.as_f64().unwrap_or(0.0);
            frame.push(Value::F64(ff(av, bv)));
        }
    }
}

fn bin_float(frame: &mut Frame, f: impl Fn(f64, f64) -> f64) {
    let r = frame.pop();
    let l = frame.pop();
    frame.push(Value::F64(f(l.as_f64().unwrap_or(0.0), r.as_f64().unwrap_or(0.0))));
}

fn native_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Bytes(b)) => String::from_utf8_lossy(b).to_string(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

fn native_bytes(v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::String(s)) => s.bytes().collect(),
        Some(Value::Hex(h, _)) => h.to_le_bytes().to_vec(),
        Some(Value::I64(i)) => i.to_le_bytes().to_vec(),
        Some(v) => v.to_string().into_bytes(),
        None => Vec::new(),
    }
}

fn format_rak(fmt: &str, args: &[Value]) -> String {
    let mut result = String::new();
    let mut idx = 0;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') { chars.next(); result.push('{'); continue; }
            let mut spec = String::new();
            while let Some(c2) = chars.next() { if c2 == '}' { break; } spec.push(c2); }
            if idx < args.len() {
                let a = &args[idx];
                if spec.contains(":04X") {
                    result.push_str(&format!("{:04X}", a.as_u64().unwrap_or(0)));
                } else if spec.contains(":08X") {
                    result.push_str(&format!("{:08X}", a.as_u64().unwrap_or(0)));
                } else if spec.contains('X') || spec.contains('x') {
                    result.push_str(&format!("{:X}", a.as_u64().unwrap_or(0)));
                } else {
                    result.push_str(&a.to_string());
                }
                idx += 1;
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') { chars.next(); result.push('}'); }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile_module;

    fn run(src: &str) -> Vec<String> {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let module = crate::parser::parse(&tokens).unwrap();
        let chunk = compile_module(&module).unwrap();
        let mut vm = Vm::new();
        vm.run(&chunk).unwrap()
    }

    #[test]
    fn test_vm_arith() {
        let out = run("dump 2 + 3 * 4");
        assert!(out.iter().any(|l| l.contains("[DUMP] 14")));
    }

    #[test]
    fn test_vm_while_loop() {
        let out = run("let i = 0; let s = 0; while i < 10 { s = s + i; i = i + 1 } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 45")));
    }

    #[test]
    fn test_vm_function() {
        let out = run("fn add(a, b) { return a + b } dump add(3, 4)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 7")));
    }

    #[test]
    fn test_vm_for_range() {
        let out = run("let s = 0; for i in 1..100 { s = s + i } dump s");
        assert!(out.iter().any(|l| l.contains("[DUMP] 5050")));
    }

    #[test]
    fn test_vm_fib() {
        let out = run("fn fib(n) { if n < 2 { return n } return fib(n - 1) + fib(n - 2) } dump fib(20)");
        assert!(out.iter().any(|l| l.contains("[DUMP] 6765")));
    }
}
