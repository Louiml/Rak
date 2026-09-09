'use client';

import React, { useRef, useEffect, useState, useCallback } from 'react';
import EditorContextMenu from './EditorContextMenu';
import { Icon, IconName } from './Icon';

interface CodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  onRun?: () => void;
  onCursorChange?: (pos: { line: number; col: number }) => void;
  fontSize?: number;
  tabSize?: number;
  autoClose?: boolean;
}

interface Snippet { trigger: string; label: string; body: string; }

const SNIPPETS: Snippet[] = [
  { trigger: 'fn', label: 'function', body: 'fn name(params) {\n    \n}' },
  { trigger: 'if', label: 'if', body: 'if cond {\n    \n}' },
  { trigger: 'for', label: 'for', body: 'for item in iterable {\n    \n}' },
  { trigger: 'while', label: 'while', body: 'while cond {\n    \n}' },
  { trigger: 'match', label: 'match', body: 'match value {\n    pattern => {\n        \n    },\n    _ => {}\n}' },
  { trigger: 'matchb', label: 'match bytes', body: 'match data {\n    [0x89, ..] => {\n        \n    },\n    _ => {}\n}' },
  { trigger: 'struct', label: 'struct', body: 'struct Name {\n    field: type\n}' },
  { trigger: 'enum', label: 'enum', body: 'enum Name {\n    Variant\n}' },
  { trigger: 'let', label: 'let', body: 'let name = value' },
  { trigger: 'pipe', label: 'pipeline', body: 'value |> fn' },
  { trigger: 'regex', label: 'regex literal', body: '/\\d+/g' },
  { trigger: 'impld', label: 'impl Display', body: 'impl Display for Name {\n    fn fmt(self) {\n        return fmt("{}", self)\n    }\n}' },
  { trigger: 'impli', label: 'impl Iterable', body: 'impl Iterable for Name {\n    fn iter(self) {\n        return []\n    }\n}' },
  { trigger: 'implx', label: 'impl Index', body: 'impl Index for Name {\n    fn index(self, key) {\n        return self.data[key]\n    }\n}' },
  { trigger: 'import', label: 'import module', body: 'import module' },
  { trigger: 'from', label: 'from import', body: 'from module import name' },
  { trigger: 'export', label: 'export fn', body: 'export fn name(params) {\n    \n}' },
  { trigger: 'macro', label: 'macro', body: 'macro name(x: expr) {\n    $x\n}' },
  { trigger: 'extern', label: 'extern C', body: 'extern "C" {\n    fn name(args) -> i32\n}' },
  { trigger: 'const', label: 'const', body: 'const NAME = value' },
  { trigger: 'async', label: 'async fn', body: 'async fn name(args) {\n    let r = await expr\n    return r\n}' },
  { trigger: 'mmap', label: 'mmap open', body: 'let m = mmap_open("file", "r")\ndump mmap_size(m)' },
  { trigger: 'netraw', label: 'net_raw SYN', body: 'let pkt = net_raw_tcp_syn("10.0.0.1", "10.0.0.2", 12345, 80)\ndump len(pkt)' },
  { trigger: 'ffi', label: 'ffi_load', body: 'let lib = ffi_load("libc.so.6")\ndump lib.call("abs", [-9])' },
  { trigger: 'dns', label: 'dns_query', body: 'dump dns_query("example.com", "A")' },
];

const KEYWORDS = [
  'scan', 'fetch', 'dump', 'trace', 'loop', 'if', 'else', 'fn', 'let', 'mut',
  'return', 'use', 'mod', 'pub', 'struct', 'enum', 'impl', 'match', 'for',
  'while', 'break', 'continue', 'true', 'false', 'nil', 'in',
  'try', 'catch', 'raise', 'throw', 'trait', 'async', 'await', 'spawn', 'as', 'type',
  'import', 'from', 'export', 'macro', 'macro_rules', 'const', 'extern',
];

const KEYWORD_SET = new Set(KEYWORDS);

const TYPES = [
  'hex8', 'hex16', 'hex32', 'hex64', 'int', 'string', 'bytes', 'bool',
  'i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'f32', 'f64',
  'Option', 'Result', 'void',
];
const TYPE_SET = new Set(TYPES);

const BUILTINS = [
  'fmt', 'md5', 'sha1', 'sha256', 'xor', 'rot13', 'hex_encode', 'hex_decode',
  'base64_encode', 'base64_decode', 'url_encode', 'url_decode', 'dns_lookup',
  'subdomain_enum', 'reverse_dns', 'len', 'split', 'join', 'contains',
  'to_hex', 'from_hex', 'int', 'string', 'bytes', 'float', 'upper', 'lower', 'trim',
  'push', 'read', 'write', 'array', 'map', 'keys', 'values', 'has', 'get', 'sort',
  'file_read', 'file_write', 'file_append', 'file_exists', 'file_size',
  'file_list', 'file_delete', 'file_mkdir', 'file_copy', 'file_rename',
  'file_ext', 'file_basename', 'file_dirname',
  'html_title', 'html_select', 'html_select_all', 'html_attr', 'html_links',
  'html_images', 'html_scripts', 'html_forms', 'html_inputs', 'html_meta',
  'html_count', 'html_headers',
  'json_parse', 'json_get', 'json_path', 'json_keys', 'json_len', 'json_find_all',
  'scan_ports', 'scan_subdomains',
  'regex_new', 'regex_match', 'regex_is_match', 'regex_find', 'regex_find_all', 'regex_replace',
  'net_listen', 'net_accept', 'net_connect', 'net_local_addr',
  'tcp_read', 'tcp_write', 'tcp_read_line', 'tcp_close',
  'spawn', 'thread_join', 'channel', 'chan_send', 'chan_recv',
  'sleep', 'now_ms', 'args', 'env_get', 'ord', 'chr', 'substr', 'print', 'dbg', 'exit',
  'Some', 'None', 'Ok', 'Err',
  // FFI
  'ffi_load', 'ffi_ptr', 'ffi_alloc', 'ffi_free', 'ffi_write', 'ffi_read',
  'ffi_read_i32', 'ffi_cstr_to_string', 'ffi_string_to_cstr', 'ffi_call',
  // Memory-mapped files
  'mmap_open', 'mmap_slice', 'mmap_size', 'mmap_close', 'mmap_find',
  'mmap_lines', 'mmap_lines_off',
  // Raw sockets
  'net_raw_csum', 'net_raw_ipv4', 'net_raw_tcp', 'net_raw_udp',
  'net_raw_tcp_syn', 'net_raw_send', 'net_raw_recv',
  // Protocol parsers
  'dns_query', 'dns_build', 'dns_parse',
  'tls_parse_client_hello', 'tls_parse_cert_chain',
  'pcap_open', 'pcap_next',
  // Async I/O
  'http_get_async', 'tcp_probe', 'tcp_connect_async',
];

interface Suggestion {
  label: string;
  kind: 'keyword' | 'type' | 'builtin' | 'snippet';
  body?: string;
}

const ALL_SUGGESTIONS: Suggestion[] = [
  ...SNIPPETS.map((s) => ({ label: s.trigger, kind: 'snippet' as const, body: s.body })),
  ...KEYWORDS.map((k) => ({ label: k, kind: 'keyword' as const })),
  ...TYPES.map((t) => ({ label: t, kind: 'type' as const })),
  ...BUILTINS.map((b) => ({ label: b, kind: 'builtin' as const })),
];

const KIND_ICON: Record<Suggestion['kind'], IconName> = { keyword: 'key', type: 'box', builtin: 'zap', snippet: 'file-code' };

interface Token {
  type: 'keyword' | 'type' | 'hex' | 'number' | 'float' | 'typedint' | 'interp' | 'string' | 'comment' | 'bytes' | 'ident' | 'op' | 'ws' | 'regex' | 'char' | 'macrovar' | 'macroinv';
  value: string;
}

function canEndExpr(t: Token | null): boolean {
  if (!t) return false; // start of line -> regex context
  if (['ident', 'hex', 'number', 'float', 'typedint', 'interp', 'string', 'bytes', 'regex', 'char', 'macrovar', 'macroinv'].includes(t.type)) return true;
  if (t.type === 'op' && (t.value === ')' || t.value === ']' || t.value === '}')) return true;
  return false;
}

function tokenizeLine(line: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  const lastSig = (): Token | null => {
    for (let k = tokens.length - 1; k >= 0; k--) if (tokens[k].type !== 'ws') return tokens[k];
    return null;
  };

  while (i < line.length) {
    if (/\s/.test(line[i])) {
      let ws = '';
      while (i < line.length && /\s/.test(line[i])) { ws += line[i]; i++; }
      tokens.push({ type: 'ws', value: ws });
      continue;
    }
    if (line[i] === '/' && line[i + 1] === '/') {
      tokens.push({ type: 'comment', value: line.slice(i) });
      break;
    }
    if (line[i] === '/' && !canEndExpr(lastSig())) {
      // Regex literal /pattern/flags (operand context).
      let j = i + 1;
      let re = '/';
      let inClass = false;
      while (j < line.length) {
        const c = line[j];
        if (c === '\\' && j + 1 < line.length) { re += c + line[j + 1]; j += 2; continue; }
        if (c === '[') { inClass = true; re += c; j++; continue; }
        if (c === ']') { inClass = false; re += c; j++; continue; }
        if (c === '/' && !inClass) { re += '/'; j++; break; }
        re += c; j++;
      }
      while (j < line.length && /[a-z]/i.test(line[j])) { re += line[j]; j++; }
      tokens.push({ type: 'regex', value: re });
      i = j;
      continue;
    }
    if (line[i] === '$' && /[a-zA-Z_]/.test(line[i + 1] || '')) {
      // Macro placeholder $name
      let mv = '$';
      i++;
      while (i < line.length && /[a-zA-Z0-9_]/.test(line[i])) { mv += line[i]; i++; }
      tokens.push({ type: 'macrovar', value: mv });
      continue;
    }
    if (line[i] === "'") {
      // Char literal 'P', '\x41', '\n'
      let j = i + 1;
      let ch = "'";
      if (line[j] === '\\' && line[j + 1] === 'x' && j + 3 < line.length && /[0-9a-fA-F]{2}/.test(line.slice(j + 2, j + 4))) {
        ch += line.slice(j, j + 4); j += 4;
      } else if (line[j] === '\\' && j + 1 < line.length) {
        ch += line[j] + line[j + 1]; j += 2;
      } else if (line[j] && line[j] !== "'") {
        ch += line[j]; j += 1;
      }
      if (line[j] === "'") { ch += "'"; j++; }
      tokens.push({ type: 'char', value: ch });
      i = j;
      continue;
    }
    if (line[i] === '0' && (line[i + 1] === 'x' || line[i + 1] === 'X')) {
      let hex = '0x';
      i += 2;
      while (i < line.length && /[0-9A-Fa-f]/.test(line[i])) { hex += line[i]; i++; }
      tokens.push({ type: 'hex', value: hex });
      continue;
    }
    if (line[i] === 'f' && line[i + 1] === '"') {
      let str = 'f"';
      i += 2;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'interp', value: str });
      continue;
    }
    if (/[0-9]/.test(line[i])) {
      let num = '';
      while (i < line.length && /[0-9]/.test(line[i])) { num += line[i]; i++; }
      if (line[i] === '.' && /[0-9]/.test(line[i + 1] || '')) {
        num += '.';
        i++;
        while (i < line.length && /[0-9]/.test(line[i])) { num += line[i]; i++; }
        if (line[i] === 'f' && (line[i + 1] === '3' || line[i + 1] === '6')) { num += 'f' + line[i + 1]; i += 2; }
        tokens.push({ type: 'float', value: num });
        continue;
      }
      const suf = line.slice(i).match(/^(i8|i16|i32|i64|u8|u16|u32|u64|f32|f64)/);
      if (suf) { num += suf[0]; i += suf[0].length; tokens.push({ type: 'typedint', value: num }); continue; }
      tokens.push({ type: 'number', value: num });
      continue;
    }
    if (line[i] === 'b' && line[i + 1] === '"') {
      let str = 'b"';
      i += 2;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'bytes', value: str });
      continue;
    }
    if (line[i] === '"') {
      let str = '"';
      i++;
      while (i < line.length && line[i] !== '"') {
        if (line[i] === '\\' && i + 1 < line.length) { str += line[i] + line[i + 1]; i += 2; }
        else { str += line[i]; i++; }
      }
      if (i < line.length) { str += '"'; i++; }
      tokens.push({ type: 'string', value: str });
      continue;
    }
    if (/[a-zA-Z_]/.test(line[i])) {
      let ident = '';
      while (i < line.length && /[a-zA-Z0-9_]/.test(line[i])) { ident += line[i]; i++; }
      // Macro invocation `name!(...)` — but not `name != ...`
      if (line[i] === '!' && line[i + 1] !== '=') { ident += '!'; i++; tokens.push({ type: 'macroinv', value: ident }); continue; }
      if (KEYWORD_SET.has(ident)) tokens.push({ type: 'keyword', value: ident });
      else if (TYPE_SET.has(ident)) tokens.push({ type: 'type', value: ident });
      else tokens.push({ type: 'ident', value: ident });
      continue;
    }
    const threeChar = line.slice(i, i + 3);
    if (['...'].includes(threeChar)) {
      tokens.push({ type: 'op', value: threeChar }); i += 3; continue;
    }
    const twoChar = line.slice(i, i + 2);
    if (['==', '!=', '<=', '>=', '<<', '>>', '&&', '||', '->', '=>', '+=', '-=', '*=', '/=', '%=', '::', '..', '|>'].includes(twoChar)) {
      tokens.push({ type: 'op', value: twoChar }); i += 2; continue;
    }
    if ('+-*/%&|^!~<>=.,:;()[]{}?'.includes(line[i])) {
      tokens.push({ type: 'op', value: line[i] }); i++; continue;
    }
    tokens.push({ type: 'ident', value: line[i] }); i++;
  }
  return tokens;
}

function getColorClass(type: Token['type']): string {
  switch (type) {
    case 'keyword': return 'text-purple-400 font-semibold';
    case 'type': return 'text-cyan-400';
    case 'hex': return 'text-orange-400 font-semibold';
    case 'number': return 'text-orange-300';
    case 'float': return 'text-orange-300';
    case 'typedint': return 'text-orange-300';
    case 'interp': return 'text-green-400';
    case 'string': return 'text-green-400';
    case 'regex': return 'text-rose-400';
    case 'char': return 'text-amber-400';
    case 'bytes': return 'text-yellow-400';
    case 'macrovar': return 'text-sky-300';
    case 'macroinv': return 'text-emerald-300 font-semibold';
    case 'comment': return 'text-zinc-600 italic';
    case 'op': return 'text-pink-400';
    case 'ident': return 'text-zinc-200';
    default: return 'text-zinc-300';
  }
}

function renderLine(line: string): React.ReactNode {
  if (line.length === 0) return '\u200B';
  const tokens = tokenizeLine(line);
  return tokens.map((token, i) => (
    <span key={i} className={getColorClass(token.type)}>{token.value}</span>
  ));
}

export default function CodeEditor({ value, onChange, onRun, onCursorChange, fontSize = 14, tabSize = 4, autoClose = true }: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLPreElement>(null);
  const lines = value.split('\n');

  // Find/Replace state
  const [findOpen, setFindOpen] = useState(false);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [replaceQuery, setReplaceQuery] = useState('');
  const [matchCount, setMatchCount] = useState(0);
  const [currentMatch, setCurrentMatch] = useState(0);
  const findInputRef = useRef<HTMLInputElement>(null);

  // Autocomplete state
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [suggestionIndex, setSuggestionIndex] = useState(0);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [suggestionTop, setSuggestionTop] = useState(0);
  const [suggestionLeft, setSuggestionLeft] = useState(0);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  const syncScroll = () => {
    if (textareaRef.current && highlightRef.current) {
      highlightRef.current.scrollTop = textareaRef.current.scrollTop;
      highlightRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  };

  // Find/replace logic
  const findMatches = useCallback((query: string) => {
    if (!query) { setMatchCount(0); setCurrentMatch(0); return; }
    let count = 0;
    let pos = 0;
    while ((pos = value.indexOf(query, pos)) !== -1) { count++; pos += query.length; }
    setMatchCount(count);
    setCurrentMatch(count > 0 ? 1 : 0);
  }, [value]);

  useEffect(() => {
    if (findQuery) findMatches(findQuery);
  }, [findQuery, value, findMatches]);

  const findNext = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const start = ta.selectionEnd;
    let pos = value.indexOf(findQuery, start);
    if (pos === -1) pos = value.indexOf(findQuery, 0);
    if (pos !== -1) {
      ta.focus();
      ta.selectionStart = pos;
      ta.selectionEnd = pos + findQuery.length;
      const matches = value.split(findQuery).length - 1;
      const current = value.substring(0, pos).split(findQuery).length;
      setCurrentMatch(current > matches ? 1 : current);
    }
  };

  const findPrev = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const start = ta.selectionStart - 1;
    let pos = value.lastIndexOf(findQuery, start);
    if (pos === -1) pos = value.lastIndexOf(findQuery);
    if (pos !== -1) {
      ta.focus();
      ta.selectionStart = pos;
      ta.selectionEnd = pos + findQuery.length;
    }
  };

  const replaceNext = () => {
    if (!findQuery || !textareaRef.current) return;
    const ta = textareaRef.current;
    const selected = ta.value.substring(ta.selectionStart, ta.selectionEnd);
    if (selected === findQuery) {
      const newValue = value.substring(0, ta.selectionStart) + replaceQuery + value.substring(ta.selectionEnd);
      onChange(newValue);
      setTimeout(() => {
        if (textareaRef.current) {
          const pos = ta.selectionStart + replaceQuery.length;
          textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
        }
      }, 0);
    }
    findNext();
  };

  const replaceAll = () => {
    if (!findQuery) return;
    const newValue = value.split(findQuery).join(replaceQuery);
    onChange(newValue);
    setMatchCount(0);
  };

  // Autocomplete logic
  const updateSuggestions = useCallback(() => {
    if (!textareaRef.current) return;
    const ta = textareaRef.current;
    const pos = ta.selectionStart;
    const before = value.substring(0, pos);
    const wordMatch = before.match(/[a-zA-Z_][a-zA-Z0-9_]*$/);
    if (wordMatch && wordMatch[0].length >= 1) {
      const prefix = wordMatch[0].toLowerCase();
      const matches = ALL_SUGGESTIONS.filter(s => s.label.toLowerCase().startsWith(prefix)).slice(0, 8);
      if (matches.length > 0 && (matches.length > 1 || matches[0].label !== wordMatch[0])) {
        setSuggestions(matches);
        setSuggestionIndex(0);
        setShowSuggestions(true);
        // compute caret pixel position (monospace: text-sm 14px, leading-6 24px, p-4)
        const lines = before.split('\n');
        const lineIdx = lines.length - 1;
        const col = lines[lineIdx].length;
        const scrollTop = ta.scrollTop;
        const scrollLeft = ta.scrollLeft;
        setSuggestionTop(16 + (lineIdx + 1) * 24 - scrollTop);
        setSuggestionLeft(16 + (col + 1) * 8.4 - scrollLeft);
        return;
      }
    }
    setShowSuggestions(false);
  }, [value]);

  const insertSuggestion = (suggestion: Suggestion) => {
    if (!textareaRef.current) return;
    const ta = textareaRef.current;
    const pos = ta.selectionStart;
    const before = value.substring(0, pos);
    const after = value.substring(pos);
    const wordMatch = before.match(/[a-zA-Z_][a-zA-Z0-9_]*$/);
    if (!wordMatch) return;
    const newBefore = before.substring(0, before.length - wordMatch[0].length);
    const inserted = suggestion.body ?? suggestion.label;
    const newValue = newBefore + inserted + after;
    onChange(newValue);
    // Position cursor inside snippet body (between first "{" matching "}" or at end)
    let cursorOffset = newBefore.length + inserted.length;
    const openBrace = inserted.indexOf('{');
    if (suggestion.body && openBrace !== -1 && inserted.indexOf('\n') > openBrace) {
      const firstNewlineAfter = inserted.indexOf('\n', openBrace);
      cursorOffset = newBefore.length + firstNewlineAfter + 1 + 4; // after indent
    }
    setTimeout(() => {
      if (textareaRef.current) {
        textareaRef.current.selectionStart = textareaRef.current.selectionEnd = cursorOffset;
        textareaRef.current.focus();
      }
    }, 0);
    setShowSuggestions(false);
  };

  const reportCursor = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta || !onCursorChange) return;
    const before = value.substring(0, ta.selectionStart);
    const lines = before.split('\n');
    onCursorChange({ line: lines.length, col: lines[lines.length - 1].length + 1 });
  }, [value, onCursorChange]);

  const getLineBounds = (): { start: number; end: number } => {
    const start = textareaRef.current?.selectionStart ?? 0;
    const lineStart = value.lastIndexOf('\n', start - 1) + 1;
    let lineEnd = value.indexOf('\n', start);
    if (lineEnd === -1) lineEnd = value.length;
    return { start: lineStart, end: lineEnd };
  };

  const toggleComment = () => {
    const { start, end } = getLineBounds();
    const line = value.substring(start, end);
    if (line.trimStart().startsWith('//')) {
      const newLine = line.replace(/^\s*\/\/\s?/, '');
      onChange(value.substring(0, start) + newLine + value.substring(end));
    } else {
      onChange(value.substring(0, start) + '// ' + line + value.substring(end));
    }
  };

  const duplicateLine = () => {
    const { start, end } = getLineBounds();
    const line = value.substring(start, end);
    onChange(value.substring(0, start) + line + '\n' + line + value.substring(end));
  };

  const deleteLine = () => {
    const { start, end } = getLineBounds();
    const newValue = value.substring(0, start) + value.substring(Math.min(end + 1, value.length));
    onChange(newValue);
  };

  const moveLine = (dir: number) => {
    const lines = value.split('\n');
    const ta = textareaRef.current;
    if (!ta) return;
    const before = value.substring(0, ta.selectionStart);
    const lineIdx = before.split('\n').length - 1;
    const target = lineIdx + dir;
    if (target < 0 || target >= lines.length) return;
    const [line] = lines.splice(lineIdx, 1);
    lines.splice(target, 0, line);
    onChange(lines.join('\n'));
  };

  const goToLine = (n: number) => {
    const lines = value.split('\n');
    const idx = Math.max(0, Math.min(n - 1, lines.length - 1));
    const charPos = lines.slice(0, idx).join('\n').length + (idx > 0 ? 1 : 0);
    const ta = textareaRef.current;
    if (ta) {
      ta.focus();
      ta.selectionStart = ta.selectionEnd = charPos;
      const lineHeight = 24;
      ta.scrollTop = Math.max(0, idx * lineHeight);
      syncScroll();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const mod = e.ctrlKey || e.metaKey;
    // Ctrl+/ - Toggle comment
    if (mod && e.key === '/') {
      e.preventDefault();
      toggleComment();
      return;
    }
    // Ctrl+D - Duplicate line
    if (mod && e.key === 'd' && !e.shiftKey) {
      e.preventDefault();
      duplicateLine();
      return;
    }
    // Ctrl+Shift+K - Delete line
    if (mod && e.shiftKey && e.key === 'K') {
      e.preventDefault();
      deleteLine();
      return;
    }
    // Alt+Up / Alt+Down - Move line
    if (e.altKey && e.key === 'ArrowUp') {
      e.preventDefault();
      moveLine(-1);
      return;
    }
    if (e.altKey && e.key === 'ArrowDown') {
      e.preventDefault();
      moveLine(1);
      return;
    }
    // Ctrl+G - Go to line
    if (mod && e.key === 'g' && !e.shiftKey) {
      e.preventDefault();
      const n = prompt('Go to line:');
      if (n && !isNaN(Number(n))) {
        goToLine(Number(n));
      }
      return;
    }
    // Auto-close brackets and quotes
    const autoCloseMap: Record<string, string> = { '(': ')', '[': ']', '{': '}', '"': '"', "'": "'" };
    if (autoClose && !mod && !e.altKey && autoCloseMap[e.key] && !e.key.startsWith('Arrow')) {
      const start = e.currentTarget.selectionStart;
      const end = e.currentTarget.selectionEnd;
      if (start === end) {
        e.preventDefault();
        const insert = e.key + autoCloseMap[e.key];
        onChange(value.substring(0, start) + insert + value.substring(end));
        setTimeout(() => {
          if (textareaRef.current) {
            textareaRef.current.selectionStart = textareaRef.current.selectionEnd = start + 1;
          }
        }, 0);
        return;
      }
    }
    // Ctrl+F - Find
    if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
      e.preventDefault();
      setFindOpen(true);
      setReplaceOpen(false);
      setTimeout(() => findInputRef.current?.focus(), 0);
      return;
    }
    // Ctrl+H - Replace
    if ((e.ctrlKey || e.metaKey) && e.key === 'h') {
      e.preventDefault();
      setFindOpen(true);
      setReplaceOpen(true);
      setTimeout(() => findInputRef.current?.focus(), 0);
      return;
    }
    // Escape - close find/replace or suggestions
    if (e.key === 'Escape') {
      setFindOpen(false);
      setShowSuggestions(false);
      return;
    }
    // Autocomplete navigation
    if (showSuggestions && suggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSuggestionIndex(prev => (prev + 1) % suggestions.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSuggestionIndex(prev => (prev - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertSuggestion(suggestions[suggestionIndex]);
        return;
      }
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      const start = e.currentTarget.selectionStart;
      const end = e.currentTarget.selectionEnd;
      const pad = ' '.repeat(tabSize);
      onChange(value.substring(0, start) + pad + value.substring(end));
      setTimeout(() => {
        if (textareaRef.current) {
          textareaRef.current.selectionStart = textareaRef.current.selectionEnd = start + tabSize;
        }
      }, 0);
      return;
    }
    if (e.key === 'Enter') {
      const start = e.currentTarget.selectionStart;
      const before = value.substring(0, start);
      const currentLine = before.split('\n').pop() || '';
      const indent = currentLine.match(/^\s*/)?.[0] || '';
      const lastChar = before.trim().slice(-1);
      if (lastChar === '{') {
        e.preventDefault();
        const pad = ' '.repeat(tabSize);
        const insert = '\n' + indent + pad + '\n' + indent;
        onChange(value.substring(0, start) + insert + value.substring(e.currentTarget.selectionEnd));
        setTimeout(() => {
          if (textareaRef.current) {
            const pos = start + 1 + indent.length + tabSize;
            textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
          }
        }, 0);
      }
    }
  };

  const handleKeyUp = () => {
    updateSuggestions();
    reportCursor();
  };

  // Build highlighted content with search match highlights
  const renderHighlighted = () => {
    return lines.map((line, i) => (
      <div key={i} className="min-h-[1.5rem]">
        {renderLine(line)}
      </div>
    ));
  };

  return (
    <div className="relative flex-1 flex bg-zinc-950 overflow-hidden">
      {/* Line numbers */}
      <div className="flex flex-col py-4 px-3 text-right bg-zinc-950 border-r border-zinc-800 select-none overflow-hidden">
        {lines.map((_, i) => (
          <div key={i} className="text-xs text-zinc-600 leading-6 min-h-[1.5rem]">{i + 1}</div>
        ))}
      </div>

      {/* Editor area */}
      <div className="relative flex-1 overflow-hidden">
        {/* Find/Replace bar */}
        {findOpen && (
          <div className="absolute top-0 right-0 z-20 bg-zinc-800 border-b border-l border-zinc-700 rounded-bl-lg p-2 flex flex-col gap-2 w-80 shadow-xl">
            <div className="flex items-center gap-2">
              <input
                ref={findInputRef}
                type="text"
                value={findQuery}
                onChange={(e) => setFindQuery(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') findNext(); }}
                placeholder="Find..."
                className="flex-1 bg-zinc-900 text-zinc-200 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
              />
              <span className="text-xs text-zinc-500 min-w-[60px]">
                {matchCount > 0 ? `${currentMatch}/${matchCount}` : '0/0'}
              </span>
              <button onClick={findPrev} className="text-zinc-400 hover:text-zinc-200 px-1"><Icon name="arrow-up" size={12} /></button>
              <button onClick={findNext} className="text-zinc-400 hover:text-zinc-200 px-1"><Icon name="arrow-down" size={12} /></button>
              <button onClick={() => { setFindOpen(false); setReplaceOpen(false); }} className="text-zinc-400 hover:text-zinc-200 px-1"><Icon name="x" size={12} /></button>
            </div>
            {replaceOpen && (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={replaceQuery}
                  onChange={(e) => setReplaceQuery(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') replaceNext(); }}
                  placeholder="Replace..."
                  className="flex-1 bg-zinc-900 text-zinc-200 text-xs px-2 py-1 rounded border border-zinc-700 outline-none focus:border-emerald-500"
                />
                <button onClick={replaceNext} className="text-xs text-zinc-300 hover:text-white bg-zinc-700 px-2 py-1 rounded">Replace</button>
                <button onClick={replaceAll} className="text-xs text-zinc-300 hover:text-white bg-zinc-700 px-2 py-1 rounded">All</button>
              </div>
            )}
            {!replaceOpen && (
              <button onClick={() => setReplaceOpen(true)} className="text-xs text-zinc-500 hover:text-zinc-300 text-left">
                Toggle replace...
              </button>
            )}
          </div>
        )}

        {/* Autocomplete dropdown */}
        {showSuggestions && suggestions.length > 0 && (
          <div
            className="absolute z-20 bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl py-1 max-h-48 overflow-y-auto text-xs"
            style={{ top: suggestionTop, left: suggestionLeft, minWidth: 180 }}
          >
            {suggestions.map((s, i) => (
              <div
                key={s.label + s.kind}
                onClick={() => insertSuggestion(s)}
                className={`px-3 py-1 cursor-pointer flex items-center gap-2 ${
                  i === suggestionIndex ? 'bg-emerald-600 text-white' : 'text-zinc-300 hover:bg-zinc-700'
                }`}
              >
                <span className="w-4 flex items-center justify-center"><Icon name={KIND_ICON[s.kind]} size={12} className={i === suggestionIndex ? 'text-white' : 'text-zinc-400'} /></span>
                <span className={s.kind === 'keyword' ? 'text-purple-400' : s.kind === 'type' ? 'text-cyan-400' : s.kind === 'builtin' ? 'text-yellow-400' : 'text-green-400'}>
                  {s.label}
                </span>
                {s.kind === 'snippet' && <span className="ml-auto"><Icon name="tab" size={11} className="text-zinc-500" /></span>}
              </div>
            ))}
          </div>
        )}

        {/* Syntax highlight overlay */}
        <pre
          ref={highlightRef}
          aria-hidden="true"
          className="absolute inset-0 p-4 font-mono pointer-events-none overflow-auto whitespace-pre"
          style={{ margin: 0, fontSize, lineHeight: '24px' }}
        >
          {renderHighlighted()}
          <div className="min-h-[1.5rem]">{'\u200B'}</div>
        </pre>

        {/* Actual textarea */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onKeyUp={handleKeyUp}
          onScroll={syncScroll}
          onClick={() => { setShowSuggestions(false); reportCursor(); }}
          onSelect={reportCursor}
          onContextMenu={(e) => {
            e.preventDefault();
            setContextMenu({ x: e.clientX, y: e.clientY });
          }}
          spellCheck={false}
          className="absolute inset-0 p-4 font-mono bg-transparent text-transparent caret-emerald-400 resize-none outline-none whitespace-pre overflow-auto"
          style={{ fontSize, lineHeight: '24px', tabSize }}
        />
      </div>

      {/* Editor context menu */}
      {contextMenu && (
        <EditorContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
          items={[
            {
              label: 'Cut',
              icon: 'scissors',
              action: () => {
                const ta = textareaRef.current;
                if (ta) {
                  document.execCommand('cut');
                  onChange(ta.value);
                }
              },
            },
            {
              label: 'Copy',
              icon: 'copy',
              action: () => {
                const ta = textareaRef.current;
                if (ta) document.execCommand('copy');
              },
            },
            {
              label: 'Paste',
              icon: 'clipboard',
              action: async () => {
                const ta = textareaRef.current;
                if (ta) {
                  const text = await navigator.clipboard.readText();
                  const start = ta.selectionStart;
                  const end = ta.selectionEnd;
                  const newValue = value.substring(0, start) + text + value.substring(end);
                  onChange(newValue);
                  setTimeout(() => {
                    if (textareaRef.current) {
                      const pos = start + text.length;
                      textareaRef.current.selectionStart = textareaRef.current.selectionEnd = pos;
                    }
                  }, 0);
                }
              },
            },
            {
              label: 'Select All',
              icon: 'list',
              action: () => {
                const ta = textareaRef.current;
                if (ta) {
                  ta.focus();
                  ta.selectionStart = 0;
                  ta.selectionEnd = value.length;
                }
              },
            },
            { separator: true },
            {
              label: 'Find...',
              icon: 'search',
              action: () => {
                setFindOpen(true);
                setReplaceOpen(false);
                setTimeout(() => findInputRef.current?.focus(), 0);
              },
            },
            {
              label: 'Replace...',
              icon: 'replace',
              action: () => {
                setFindOpen(true);
                setReplaceOpen(true);
                setTimeout(() => findInputRef.current?.focus(), 0);
              },
            },
            { separator: true },
            {
              label: 'Toggle Comment',
              icon: 'message-square',
              action: toggleComment,
            },
            {
              label: 'Duplicate Line',
              icon: 'copy',
              action: duplicateLine,
            },
            {
              label: 'Go to Line...',
              icon: 'ruler',
              action: () => {
                const n = prompt('Go to line:');
                if (n && !isNaN(Number(n))) goToLine(Number(n));
              },
            },
            { separator: true },
            {
              label: 'Run Script',
              icon: 'play',
              action: () => onRun?.(),
            },
          ]}
        />
      )}
    </div>
  );
}